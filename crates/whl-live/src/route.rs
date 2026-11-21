use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::stream;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::stream::{CaptureSummary, EndpointLiveState};

use crate::event::{Event, ResyncReason};
use crate::hub::{Hub, Subscription};
use crate::sse;

pub const STREAM_PATH: &str = "/api/v1/endpoints/{id}/stream";
pub const RETRY_MILLIS: u64 = 2000;
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
pub const MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);
pub const REPLAY_LIMIT: usize = 100;

const LAST_EVENT_ID_HEADER: &str = "last-event-id";
const LAST_EVENT_ID_PARAM: &str = "last_event_id";
const RETRY_AFTER_SECONDS: &str = "5";

#[derive(Debug, Clone, Copy)]
pub struct StreamSettings {
    pub ping_interval: Duration,
    pub max_lifetime: Duration,
    pub replay_limit: usize,
    pub retention: Duration,
}

impl StreamSettings {
    pub fn new(retention: Duration) -> Self {
        Self {
            ping_interval: PING_INTERVAL,
            max_lifetime: MAX_LIFETIME,
            replay_limit: REPLAY_LIMIT,
            retention,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiveState {
    store: Store,
    hub: Arc<Hub>,
    settings: StreamSettings,
}

impl LiveState {
    pub fn new(store: Store, hub: Arc<Hub>, settings: StreamSettings) -> Self {
        Self {
            store,
            hub,
            settings,
        }
    }

    pub fn hub(&self) -> &Arc<Hub> {
        &self.hub
    }

    pub fn close_streams(&self) {
        self.hub.close_streams();
    }
}

pub fn router(state: LiveState) -> Router {
    Router::new()
        .route(STREAM_PATH, get(stream_endpoint))
        .with_state(state)
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct ReadyBody<'a> {
    endpoint_id: &'a str,
    last_seq: i64,
    capture_count: i32,
}

#[derive(Serialize)]
struct SummaryBody<'a> {
    id: &'a str,
    seq: i64,
    endpoint_id: &'a str,
    received_at: &'a str,
    method: &'a str,
    path: &'a str,
    query_string: &'a str,
    client_ip: &'a str,
    content_type: Option<&'a str>,
    body_size: i32,
    status_override: Option<i16>,
    response_status: i16,
    forward_state: Option<&'static str>,
}

fn error_response(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    let body = ErrorBody {
        error: ErrorDetail { code, message },
    };
    (
        status,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        axum::Json(body),
    )
        .into_response()
}

fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found", "not found")
}

fn gone() -> Response {
    error_response(
        StatusCode::GONE,
        "endpoint_gone",
        "the endpoint does not exist, has expired or was deleted",
    )
}

fn unavailable() -> Response {
    let mut response = error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "store_unavailable",
        "the database is not available",
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from_static(RETRY_AFTER_SECONDS),
    );
    response
}

async fn stream_endpoint(
    State(state): State<LiveState>,
    Path(raw_id): Path<String>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(endpoint) = EndpointId::parse(&raw_id) else {
        return not_found();
    };
    let resume = resume_point(&headers, query.as_deref());

    let live = match state
        .store
        .extend_endpoint_lifetime(&endpoint, state.settings.retention)
        .await
    {
        Ok(Some(live)) => live,
        Ok(None) => return gone(),
        Err(error) => {
            tracing::error!(%error, endpoint_id = %endpoint, "cannot read the endpoint for a stream");
            return unavailable();
        }
    };

    let subscription = state.hub.subscribe(&endpoint);
    let now = Instant::now();
    let mut viewer = Viewer {
        subscription,
        queue: VecDeque::new(),
        floor: i64::MIN,
        ping_interval: state.settings.ping_interval,
        next_ping: now + state.settings.ping_interval,
        deadline: now + state.settings.max_lifetime,
        finishing: false,
    };
    viewer.queue.push_back(sse::retry_frame(RETRY_MILLIS));
    if viewer.subscription.was_closing_at_subscribe() {
        viewer.push_event(&Event::Bye);
        viewer.finishing = true;
    } else {
        viewer.push_ready(&endpoint, live);
        if let Some(after) = resume {
            viewer.floor = after;
            viewer.replay(&state, &endpoint, after).await;
        }
    }

    let body = Body::from_stream(stream::unfold(viewer, next_chunk));
    stream_response(body)
}

fn stream_response(body: Body) -> Response {
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn resume_point(headers: &HeaderMap, query: Option<&str>) -> Option<i64> {
    let from_header = headers
        .get(LAST_EVENT_ID_HEADER)
        .and_then(|value| parse_cursor("Last-Event-ID header", value.to_str().ok()?));
    let from_query = query.and_then(|query| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(name, _)| *name == LAST_EVENT_ID_PARAM)
            .and_then(|(_, value)| parse_cursor("last_event_id parameter", value))
    });
    match (from_header, from_query) {
        (Some(header), Some(query)) => Some(header.max(query)),
        (header, query) => header.or(query),
    }
}

fn parse_cursor(source: &'static str, text: &str) -> Option<i64> {
    match text.trim().parse::<i64>() {
        Ok(seq) if seq >= 0 => Some(seq),
        _ => {
            tracing::info!(source, "ignored a malformed stream resume point");
            None
        }
    }
}

struct Viewer {
    subscription: Subscription,
    queue: VecDeque<Bytes>,
    floor: i64,
    ping_interval: Duration,
    next_ping: Instant,
    deadline: Instant,
    finishing: bool,
}

impl Viewer {
    fn push_event(&mut self, event: &Event) {
        self.queue.push_back(sse::event_frame(
            event.name(),
            event.event_id(),
            &event.data(),
        ));
    }

    fn push_resync(&mut self, reason: ResyncReason) {
        self.push_event(&Event::Resync { reason });
    }

    fn push_json(&mut self, name: &str, id: Option<i64>, body: &impl Serialize) {
        match serde_json::to_string(body) {
            Ok(data) => self.queue.push_back(sse::event_frame(name, id, &data)),
            Err(error) => tracing::error!(%error, event = name, "cannot encode a stream event"),
        }
    }

    fn push_ready(&mut self, endpoint: &EndpointId, live: EndpointLiveState) {
        let body = ReadyBody {
            endpoint_id: endpoint.unpack(),
            last_seq: live.last_seq,
            capture_count: live.capture_count,
        };
        self.push_json("ready", None, &body);
    }

    fn push_capture(&mut self, capture: &CaptureSummary) {
        let body = SummaryBody {
            id: &capture.id,
            seq: capture.seq,
            endpoint_id: &capture.endpoint_id,
            received_at: &capture.received_at,
            method: &capture.method,
            path: &capture.path,
            query_string: &capture.query_string,
            client_ip: &capture.client_ip,
            content_type: capture.content_type.as_deref(),
            body_size: capture.body_size,
            status_override: capture.status_override,
            response_status: capture.response_status,
            forward_state: capture.forward_state.map(|state| state.as_str()),
        };
        self.push_json("capture", Some(capture.seq), &body);
    }

    async fn replay(&mut self, state: &LiveState, endpoint: &EndpointId, after: i64) {
        let limit = state.settings.replay_limit;
        let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        match state.store.captures_after(endpoint, after, fetch).await {
            Ok(captures) if captures.len() > limit => self.push_resync(ResyncReason::Gap),
            Ok(captures) => {
                for capture in &captures {
                    self.push_capture(capture);
                    self.floor = self.floor.max(capture.seq);
                }
            }
            Err(error) => {
                tracing::error!(%error, endpoint_id = %endpoint, "cannot replay captures for a stream");
                self.push_resync(ResyncReason::Gap);
            }
        }
    }

    fn accept(&mut self, event: &Event) {
        match event {
            Event::Capture { seq, .. } if *seq <= self.floor => {}
            Event::Bye => {
                self.push_event(event);
                self.finishing = true;
            }
            _ => self.push_event(event),
        }
    }
}

async fn next_chunk(mut viewer: Viewer) -> Option<(Result<Bytes, Infallible>, Viewer)> {
    loop {
        if let Some(frame) = viewer.queue.pop_front() {
            viewer.next_ping = Instant::now() + viewer.ping_interval;
            return Some((Ok(frame), viewer));
        }
        if viewer.finishing || Instant::now() >= viewer.deadline {
            return None;
        }
        tokio::select! {
            received = viewer.subscription.recv() => match received {
                Ok(event) => viewer.accept(&event),
                Err(RecvError::Lagged(skipped)) => {
                    tracing::debug!(skipped, "a stream viewer lagged behind");
                    viewer.push_resync(ResyncReason::Lagged);
                }
                Err(RecvError::Closed) => return None,
            },
            () = tokio::time::sleep_until(viewer.next_ping) => {
                viewer.queue.push_back(sse::ping_frame());
            }
            () = tokio::time::sleep_until(viewer.deadline) => return None,
        }
    }
}
