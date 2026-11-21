use std::net::SocketAddr;
use std::time::Duration;

use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use bytes::Bytes;
use whl_core::endpoint_id::EndpointId;
use whl_core::status::{HttpStatus, status_override_from_path};
use whl_store::captures::{
    CaptureOutcome, CaptureStoreError, Endpoint, EndpointState, NameValue, NewCapture,
    RenderRecord, StoredCapture,
};

use crate::body::{ReadFailure, read_body};
use crate::client_ip::{ForwardedRequest, sanitize_host};
use crate::form::{FormLimits, parse_form, parse_query};
use crate::refusal::{Failure, Refusal};
use crate::response::{Content, ReplyRequest, build_content, build_response, is_preflight};
use crate::seams::Admission;
use crate::state::CaptureState;
use crate::template::{RenderInput, Rendering, Served};
use crate::verification::{Presented, verify_capture};
use crate::target::{Target, resolve, route};

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

    let routed = route(&parts.uri).ok_or_else(|| early(Refusal::Malformed))?;
    let sub_path = routed.sub_path;

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

    let via_alias = matches!(routed.target, Target::Alias(_));
    let id = resolve(state, routed.target).await.map_err(early)?;
    let exposed = |id: &EndpointId| (!via_alias).then(|| id.clone());

    let admission = state.store().endpoint_admission(&id, state.policy()).await;
    match admission.as_ref().map(|admission| admission.state) {
        Ok(EndpointState::Live) => {
            if let Admission::Limited { retry_after } = state.admit_endpoint(&id) {
                return Err(early(Refusal::RateLimited { retry_after }));
            }
        }
        Ok(EndpointState::Full) => return Err(early(Refusal::Full(exposed(&id)))),
        Ok(EndpointState::Gone) => return Err(early(Refusal::Gone(exposed(&id)))),
        Err(error) => return Err(early(store_refusal(error))),
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

    let received_at = state.source().now();
    let verification = match admission.ok().and_then(|admission| admission.verifier) {
        Some(source) => {
            let presented = Presented {
                headers: &parts.headers,
                body: &body,
                received_at,
            };
            Some(verify_capture(state, &id, &source, presented).await)
        }
        None => None,
    };

    let Ok(capture_id) = state.source().capture_id() else {
        tracing::error!("the capture id source failed");
        return Err(late(Refusal::Internal));
    };
    let new_capture = NewCapture {
        id: capture_id,
        endpoint_id: id.clone(),
        received_at,
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
        let mut new_capture = new_capture;
        let outcome = store
            .store_capture_retained(&mut new_capture, verification, policy)
            .await;
        (outcome, new_capture, reservation)
    });
    let (outcome, request, reservation) = match job.await {
        Ok((Ok(outcome), request, reservation)) => (outcome, request, reservation),
        Ok((Err(error), _, _)) => return Err(late(store_refusal(&error))),
        Err(error) => {
            tracing::error!(%error, "the capture task failed");
            return Err(late(Refusal::Internal));
        }
    };

    match outcome {
        CaptureOutcome::Stored { endpoint, capture } => {
            let reply = ReplyRequest {
                method: &parts.method,
                headers: &parts.headers,
                via_alias,
            };
            let response = if endpoint.templated && !is_preflight(&endpoint, &reply) {
                let response = templated_response(
                    state,
                    &endpoint,
                    &capture,
                    request,
                    status_override,
                    &reply,
                )
                .await;
                drop(reservation);
                response
            } else {
                drop(request);
                drop(reservation);
                build_response(&endpoint, &capture, status_override, &reply)
            };
            state
                .delays()
                .wait(Duration::from_millis(u64::from(endpoint.delay_ms)))
                .await;
            Ok(response)
        }
        CaptureOutcome::Gone => Err(late(Refusal::Gone(exposed(&id)))),
        CaptureOutcome::Full => Err(late(Refusal::Full(exposed(&id)))),
    }
}

async fn templated_response(
    state: &CaptureState,
    endpoint: &Endpoint,
    capture: &StoredCapture,
    request: NewCapture,
    status_override: Option<HttpStatus>,
    reply: &ReplyRequest<'_>,
) -> Response {
    let input = RenderInput {
        endpoint: endpoint.clone(),
        capture: capture.clone(),
        request,
        public_url: state.public_url().to_owned(),
        with_status: status_override.is_none(),
    };
    let rendering = state.templates().respond(state.store(), input).await;
    if let Some(code) = rendering.fallback_code() {
        tracing::warn!(
            endpoint_id = endpoint.id.unpack(),
            capture_id = %capture.id,
            code = code.as_str(),
            "a templated response fell back to the static one"
        );
    }
    record_rendering(state, capture, &rendering);
    let content = match rendering.served {
        Served::Rendered(parts) => Content::Rendered(parts),
        Served::Fallback(code) => Content::Fallback(code),
    };
    build_content(endpoint, capture, status_override, reply, content)
}

fn record_rendering(state: &CaptureState, capture: &StoredCapture, rendering: &Rendering) {
    let status = rendering
        .rendered_status()
        .filter(|status| *status != capture.response_status);
    let meta = rendering.meta();
    if status.is_none() && meta.is_none() {
        return;
    }
    let record = RenderRecord {
        meta: meta.unwrap_or_else(|| serde_json::json!({})),
        response_status: status,
    };
    let Some(slot) = state.templates().recording_slot() else {
        tracing::warn!(
            capture_id = %capture.id,
            "the template outcome was not recorded because the recorder is saturated"
        );
        return;
    };
    let store = state.store().clone();
    let id = capture.id;
    tokio::spawn(async move {
        let _slot = slot;
        if let Err(error) = store.record_render(&id, &record).await {
            tracing::warn!(%error, capture_id = %id, "the template outcome could not be recorded");
        }
    });
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
