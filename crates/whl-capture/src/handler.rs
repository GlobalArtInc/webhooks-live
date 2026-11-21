use std::net::SocketAddr;

use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use bytes::Bytes;
use whl_core::endpoint_id::{ENDPOINT_ID_LEN, EndpointId};
use whl_core::status::status_override_from_path;
use whl_store::captures::{
    CaptureOutcome, CaptureStoreError, EndpointState, NameValue, NewCapture,
};

use crate::body::{ReadFailure, read_body};
use crate::client_ip::{ForwardedRequest, sanitize_host};
use crate::form::{FormLimits, parse_form, parse_query};
use crate::refusal::{Failure, Refusal};
use crate::response::build_response;
use crate::seams::Admission;
use crate::state::CaptureState;

pub(crate) async fn capture(State(state): State<CaptureState>, request: Request) -> Response {
    match run(&state, request).await {
        Ok(response) => response,
        Err(failure) => failure.into_response(&state),
    }
}

async fn run(state: &CaptureState, request: Request) -> Result<Response, Failure> {
    let (parts, body) = request.into_parts();
    let unread = !body.is_end_stream();
    let early = |refusal: Refusal| Failure::new(refusal, unread);

    let id = endpoint_id_of(&parts.uri).ok_or_else(|| early(Refusal::Malformed))?;
    let sub_path = &parts.uri.path()[1 + ENDPOINT_ID_LEN..];

    let Some(peer) = parts.extensions.get::<ConnectInfo<SocketAddr>>() else {
        tracing::error!("the capture router is served without connection info");
        return Err(early(Refusal::Internal));
    };
    let forwarded = state.proxies().resolve(peer.0.ip(), &parts.headers);

    if let Admission::Limited { retry_after } = state.admit(forwarded.client_ip) {
        return Err(early(Refusal::RateLimited { retry_after }));
    }
    if state.storage_full().is_full() {
        return Err(early(Refusal::StorageFull));
    }

    match state.store().endpoint_state(&id, state.policy()).await {
        Ok(EndpointState::Live) => {}
        Ok(EndpointState::Full) => return Err(early(Refusal::Full(id))),
        Ok(EndpointState::Gone) => return Err(early(Refusal::Gone(id))),
        Err(error) => return Err(early(store_refusal(&error))),
    }

    let declared = declared_length(&parts.headers, &body);
    if declared.is_some_and(|length| length > state.limits().max_body_bytes as u64) {
        return Err(early(Refusal::TooLarge));
    }

    let buffered = read_body(body, declared, state.limits(), state.budget())
        .await
        .map_err(|failure| early(read_refusal(failure)))?;
    let late = |refusal: Refusal| Failure::new(refusal, false);

    let content_type = first_header_text(&parts.headers, CONTENT_TYPE.as_str());
    let body = Bytes::from(buffered.data);
    let limits = FormLimits::default();
    let form = parse_form(content_type.as_deref(), &body, &limits).await;
    let status_override = status_override_from_path(sub_path);

    let Ok(capture_id) = state.source().capture_id() else {
        tracing::error!("the capture id source failed");
        return Err(late(Refusal::Internal));
    };
    let new_capture = NewCapture {
        id: capture_id,
        endpoint_id: id.clone(),
        received_at: state.source().now(),
        method: parts.method.as_str().to_owned(),
        url: stored_url(&parts.uri, &parts.headers, &forwarded),
        path: sub_path.to_owned(),
        query_string: parts.uri.query().unwrap_or_default().to_owned(),
        host: host_of(&parts.uri, &parts.headers, &forwarded),
        client_ip: forwarded.client_ip,
        headers: header_pairs(&parts.headers),
        query: parse_query(parts.uri.query(), &limits),
        form,
        content_type,
        body,
        status_override,
    };

    let store = state.store().clone();
    let policy = state.policy();
    let reservation = buffered.reservation;
    let job = tokio::spawn(async move {
        let outcome = store.store_capture(new_capture, policy).await;
        drop(reservation);
        outcome
    });
    let outcome = match job.await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => return Err(late(store_refusal(&error))),
        Err(error) => {
            tracing::error!(%error, "the capture task failed");
            return Err(late(Refusal::Internal));
        }
    };

    match outcome {
        CaptureOutcome::Stored { endpoint, capture } => {
            Ok(build_response(&endpoint, &capture, status_override))
        }
        CaptureOutcome::Gone => Err(late(Refusal::Gone(id))),
        CaptureOutcome::Full => Err(late(Refusal::Full(id))),
    }
}

pub(crate) fn endpoint_id_of(uri: &Uri) -> Option<EndpointId> {
    let first = uri.path().strip_prefix('/')?.split('/').next()?;
    EndpointId::parse(first).ok()
}

fn store_refusal(error: &CaptureStoreError) -> Refusal {
    if error.is_unavailable() {
        tracing::warn!(%error, "the store is unavailable for a capture");
        Refusal::StoreUnavailable
    } else {
        tracing::error!(%error, "the store failed a capture");
        Refusal::Internal
    }
}

fn read_refusal(failure: ReadFailure) -> Refusal {
    match failure {
        ReadFailure::TooLarge => Refusal::TooLarge,
        ReadFailure::Timeout => Refusal::ReadTimeout,
        ReadFailure::Aborted => Refusal::BodyAborted,
        ReadFailure::BudgetExhausted => Refusal::BudgetExhausted,
    }
}

fn declared_length(headers: &HeaderMap, body: &Body) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .or_else(|| body.size_hint().exact())
}

fn first_header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

fn header_pairs(headers: &HeaderMap) -> Vec<NameValue> {
    headers
        .iter()
        .map(|(name, value)| NameValue {
            name: name.as_str().to_owned(),
            value: String::from_utf8_lossy(value.as_bytes()).into_owned(),
        })
        .collect()
}

fn host_of(uri: &Uri, headers: &HeaderMap, forwarded: &ForwardedRequest) -> String {
    if let Some(host) = &forwarded.host {
        return host.clone();
    }
    let raw = headers
        .get(HOST)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .or_else(|| uri.authority().map(|authority| authority.to_string()))
        .unwrap_or_default();
    sanitize_host(&raw)
}

fn stored_url(uri: &Uri, headers: &HeaderMap, forwarded: &ForwardedRequest) -> String {
    let target = uri
        .path_and_query()
        .map_or_else(|| uri.path(), |target| target.as_str());
    format!(
        "{}://{}{}",
        forwarded.scheme.as_str(),
        host_of(uri, headers, forwarded),
        target
    )
}
