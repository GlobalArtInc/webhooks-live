use std::net::SocketAddr;
use std::time::Duration;

use axum::body::{Body, HttpBody};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use bytes::Bytes;
use whl_core::endpoint_id::{ENDPOINT_ID_LEN, EndpointId};
use whl_core::status::{HttpStatus, status_override_from_path};
use whl_rules::MAX_DELAY_MS;
use whl_store::captures::{
    CaptureOutcome, CaptureStoreError, Endpoint, EndpointState, NameValue, NewCapture,
    RenderRecord, StoredCapture,
};
use whl_store::rule_runs::{RuleRunSeed, RunStart};

use crate::body::{ReadFailure, read_body};
use crate::client_ip::{ForwardedRequest, sanitize_host};
use crate::form::{FormLimits, parse_form, parse_query};
use crate::refusal::{Failure, Refusal};
use crate::response::{Content, ReplyRequest, Rules, build_ruled, is_preflight};
use crate::rules::{Disposition, LiveInput, RuleAdmission, RuleFacts, Slot};
use crate::seams::Admission;
use crate::state::CaptureState;
use crate::template::{RenderInput, Served};
use crate::verification::{Presented, verify_capture};

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

    let admission = state.store().endpoint_admission(&id, state.policy()).await;
    match admission.as_ref().map(|admission| admission.state) {
        Ok(EndpointState::Live) => {
            if let Admission::Limited { retry_after } = state.admit_endpoint(&id) {
                return Err(early(Refusal::RateLimited { retry_after }));
            }
        }
        Ok(EndpointState::Full) => return Err(early(Refusal::Full(id))),
        Ok(EndpointState::Gone) => {
            state.rules().forget(id.unpack());
            return Err(early(Refusal::Gone(id)));
        }
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
            };
            let preflight = is_preflight(&endpoint, &reply);
            let sync_run = match capture.rule_run {
                Some(seed) if seed.start == RunStart::Sync => {
                    match state.rules().admit(endpoint.id.unpack()) {
                        RuleAdmission::Granted(slot) => Some(SyncRun::Granted {
                            seed,
                            slot,
                            facts: Box::new(RuleFacts::from_capture(
                                &request,
                                &capture,
                                &endpoint,
                                state.public_url(),
                                verification,
                            )),
                        }),
                        RuleAdmission::Full => Some(SyncRun::Refused),
                        RuleAdmission::Off => None,
                    }
                }
                _ => None,
            };
            let (content, rendered) = if endpoint.templated && !preflight {
                templated_content(state, &endpoint, &capture, request, status_override).await
            } else {
                drop(request);
                (Content::Configured, RenderedRecord::default())
            };
            drop(reservation);
            let disposition = match sync_run {
                Some(SyncRun::Granted { seed, slot, facts }) => {
                    state
                        .rules()
                        .before_response(
                            state.store(),
                            LiveInput {
                                endpoint: &endpoint,
                                capture: &capture,
                                seed,
                                retention: state.limits().retention,
                                status_override: status_override.map(|status| status.unpack()),
                                preflight,
                                keyring: state.secrets(),
                            },
                            *facts,
                            slot,
                        )
                        .await
                }
                Some(SyncRun::Refused) => state.rules().refuse(state.store(), capture.id),
                None => Disposition::Untouched,
            };
            let rule_status =
                matches!(&disposition, Disposition::Applied(draft) if draft.status.is_some());
            record_rendering(state, &capture, rendered, !rule_status);
            let (rules, extra_delay) = match &disposition {
                Disposition::Applied(draft) => (Rules::Applied(draft), draft.delay_ms),
                Disposition::Failed(code) => (Rules::Failed(*code), 0),
                Disposition::Untouched => (Rules::None, 0),
            };
            let response =
                build_ruled(&endpoint, &capture, status_override, &reply, content, rules);
            state
                .delays()
                .wait(total_delay(endpoint.delay_ms, extra_delay))
                .await;
            Ok(response)
        }
        CaptureOutcome::Gone => {
            state.rules().forget(id.unpack());
            Err(late(Refusal::Gone(id)))
        }
        CaptureOutcome::Full => Err(late(Refusal::Full(id))),
    }
}

enum SyncRun {
    Granted {
        seed: RuleRunSeed,
        slot: Slot,
        facts: Box<RuleFacts>,
    },
    Refused,
}

fn total_delay(configured: u32, extra: u32) -> Duration {
    if extra == 0 {
        return Duration::from_millis(u64::from(configured));
    }
    let cap = u32::try_from(MAX_DELAY_MS).unwrap_or(u32::MAX);
    Duration::from_millis(u64::from(configured.saturating_add(extra).min(cap)))
}

async fn templated_content(
    state: &CaptureState,
    endpoint: &Endpoint,
    capture: &StoredCapture,
    request: NewCapture,
    status_override: Option<HttpStatus>,
) -> (Content, RenderedRecord) {
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
    let record = RenderedRecord {
        meta: rendering.meta(),
        status: rendering.rendered_status(),
    };
    let content = match rendering.served {
        Served::Rendered(parts) => Content::Rendered(parts),
        Served::Fallback(code) => Content::Fallback(code),
    };
    (content, record)
}

#[derive(Default)]
struct RenderedRecord {
    meta: Option<serde_json::Value>,
    status: Option<HttpStatus>,
}

fn record_rendering(
    state: &CaptureState,
    capture: &StoredCapture,
    rendered: RenderedRecord,
    keep_status: bool,
) {
    let status = rendered
        .status
        .filter(|status| keep_status && *status != capture.response_status);
    let meta = rendered.meta;
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
