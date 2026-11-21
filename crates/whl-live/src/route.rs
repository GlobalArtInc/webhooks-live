use std::collections::VecDeque;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::time::Instant;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_store::stream::{CaptureSummary, EndpointLiveState};

use crate::emit::{self, ConnectionGauge};
use crate::event::{Event, Outcome, ResyncReason};
use crate::guard::WRITE_TIMEOUT;
use crate::hub::{Hub, Subscription};
use crate::{jitter, load, sse};

pub const STREAM_PATH: &str = "/api/v1/endpoints/{id}/stream";
pub const RETRY_MILLIS: u64 = 2000;
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
pub const MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);
pub const REPLAY_LIMIT: usize = 100;
pub const LIFETIME_JITTER: f64 = 0.1;
pub const DEFAULT_MAX_CAPTURES: u32 = 500;

const OUTBOX_FRAMES: usize = 8;
const LAST_EVENT_ID_HEADER: &str = "last-event-id";
const LAST_EVENT_ID_PARAM: &str = "last_event_id";
const RETRY_AFTER_SECONDS: &str = "5";

#[derive(Debug, Clone)]
pub struct StreamSettings {
    pub ping_interval: Duration,
    pub max_lifetime: Duration,
    pub replay_limit: usize,
    pub retention: Duration,
    pub write_timeout: Duration,
    pub public_url: String,
    pub max_captures: u32,
}

impl StreamSettings {
    pub fn new(retention: Duration) -> Self {
        Self {
            ping_interval: PING_INTERVAL,
            max_lifetime: MAX_LIFETIME,
            replay_limit: REPLAY_LIMIT,
            retention,
            write_timeout: WRITE_TIMEOUT,
            public_url: String::new(),
            max_captures: DEFAULT_MAX_CAPTURES,
        }
    }

    pub fn with_instance(mut self, public_url: impl Into<String>, max_captures: u32) -> Self {
        self.public_url = public_url.into();
        self.max_captures = max_captures;
        self
    }
}

#[derive(Debug, Clone)]
pub struct LiveState {
    pub(crate) store: Store,
    pub(crate) hub: Arc<Hub>,
    pub(crate) settings: Arc<StreamSettings>,
}

impl LiveState {
    pub fn new(store: Store, hub: Arc<Hub>, settings: StreamSettings) -> Self {
        Self {
            store,
            hub,
            settings: Arc::new(settings),
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
    let mut viewer = Viewer::new(endpoint.clone(), subscription, &state.settings);
    viewer.queue.push_back(sse::retry_frame(RETRY_MILLIS));
    if viewer.subscription.was_closing_at_subscribe() {
        viewer.push_event(&Event::Bye);
        viewer.finishing = true;
    } else {
        viewer.push_ready(&endpoint, live);
        if let Some(after) = resume {
            viewer.resume(&state, after, live.last_seq).await;
        }
    }

    let (outbox, frames) = mpsc::channel(OUTBOX_FRAMES);
    tokio::spawn(pump(viewer, outbox, state));
    stream_response(Body::from_stream(FrameStream { frames }))
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

struct FrameStream {
    frames: mpsc::Receiver<Bytes>,
}

impl Stream for FrameStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.frames.poll_recv(cx).map(|frame| frame.map(Ok))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    Finished,
    Disconnected,
    Stalled,
    HubClosed,
}

struct Viewer {
    endpoint: EndpointId,
    subscription: Subscription,
    queue: VecDeque<Bytes>,
    floor: i64,
    ping_interval: Duration,
    write_timeout: Duration,
    next_ping: Instant,
    deadline: Instant,
    finishing: bool,
    _connection: ConnectionGauge,
}

impl Viewer {
    fn new(endpoint: EndpointId, subscription: Subscription, settings: &StreamSettings) -> Self {
        let now = Instant::now();
        Self {
            endpoint,
            subscription,
            queue: VecDeque::new(),
            floor: i64::MIN,
            ping_interval: settings.ping_interval,
            write_timeout: settings.write_timeout,
            next_ping: now + settings.ping_interval,
            deadline: now + jitter::jittered(settings.max_lifetime, LIFETIME_JITTER),
            finishing: false,
            _connection: ConnectionGauge::open(),
        }
    }

    fn push_event(&mut self, event: &Event) {
        match event.data() {
            Some(data) => self.push_frame(event.name(), event.event_id(), &data),
            None => {
                tracing::error!(event = event.name(), "an unresolved event reached a stream");
            }
        }
    }

    fn push_frame(&mut self, name: &'static str, id: Option<i64>, data: &str) {
        self.queue.push_back(sse::event_frame(name, id, data));
        emit::event_sent(name);
    }

    fn push_resync(&mut self, reason: ResyncReason) {
        emit::resync_sent(reason);
        self.push_event(&Event::Resync { reason });
    }

    fn push_json(&mut self, name: &'static str, id: Option<i64>, body: &impl Serialize) {
        match serde_json::to_string(body) {
            Ok(data) => self.push_frame(name, id, &data),
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

    fn push_resolved(&mut self, event: &Event, outcome: &Outcome) {
        match outcome {
            Outcome::Data(data) => self.push_frame(event.name(), None, data.unpack()),
            Outcome::Gone => {
                tracing::debug!(event = event.name(), endpoint_id = %self.endpoint, "dropped an event whose subject no longer exists");
            }
            Outcome::Failed => self.push_resync(ResyncReason::Gap),
        }
    }

    async fn resume(&mut self, state: &LiveState, after: i64, last_seq: i64) {
        if after > last_seq {
            tracing::info!(endpoint_id = %self.endpoint, after, last_seq, "the resume point is beyond the endpoint's last seq");
            self.floor = last_seq;
            self.push_resync(ResyncReason::Gap);
            return;
        }
        self.floor = after;
        self.replay(state, after).await;
    }

    async fn replay(&mut self, state: &LiveState, after: i64) {
        let limit = state.settings.replay_limit;
        let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        match state
            .store
            .captures_after(&self.endpoint, after, fetch)
            .await
        {
            Ok(captures) if captures.len() > limit => self.push_resync(ResyncReason::Gap),
            Ok(captures) => {
                for capture in &captures {
                    self.push_capture(capture);
                    self.floor = self.floor.max(capture.seq);
                }
            }
            Err(error) => {
                tracing::error!(%error, endpoint_id = %self.endpoint, "cannot replay captures for a stream");
                self.push_resync(ResyncReason::Gap);
            }
        }
    }

    async fn accept(&mut self, event: &Event, state: &LiveState) {
        match event {
            Event::Capture { seq, .. } if *seq <= self.floor => {}
            Event::Bye => {
                self.push_event(event);
                self.finishing = true;
            }
            Event::EndpointUpdated { lookup } => {
                let outcome = lookup.resolve(load::endpoint(state, &self.endpoint)).await;
                self.push_resolved(event, outcome);
            }
            Event::ForwardResult {
                capture_id,
                job_id,
                lookup,
            } => {
                let loading = load::forward_result(state, &self.endpoint, capture_id, job_id);
                let outcome = lookup.resolve(loading).await;
                self.push_resolved(event, outcome);
            }
            _ => self.push_event(event),
        }
    }

    async fn flush(&mut self, outbox: &mpsc::Sender<Bytes>) -> Option<Ending> {
        while let Some(frame) = self.queue.pop_front() {
            match tokio::time::timeout(self.write_timeout, outbox.send(frame)).await {
                Ok(Ok(())) => self.next_ping = Instant::now() + self.ping_interval,
                Ok(Err(_)) => return Some(Ending::Disconnected),
                Err(_) => return Some(Ending::Stalled),
            }
        }
        None
    }

    async fn run(&mut self, outbox: &mpsc::Sender<Bytes>, state: &LiveState) -> Ending {
        loop {
            if let Some(ending) = self.flush(outbox).await {
                return ending;
            }
            if self.finishing || Instant::now() >= self.deadline {
                return Ending::Finished;
            }
            tokio::select! {
                received = self.subscription.recv() => match received {
                    Ok(event) => self.accept(&event, state).await,
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, "a stream viewer lagged behind");
                        self.push_resync(ResyncReason::Lagged);
                    }
                    Err(RecvError::Closed) => return Ending::HubClosed,
                },
                () = tokio::time::sleep_until(self.next_ping) => {
                    self.queue.push_back(sse::ping_frame());
                }
                () = tokio::time::sleep_until(self.deadline) => return Ending::Finished,
                () = outbox.closed() => return Ending::Disconnected,
            }
        }
    }
}

async fn pump(mut viewer: Viewer, outbox: mpsc::Sender<Bytes>, state: LiveState) {
    if viewer.run(&outbox, &state).await == Ending::Stalled {
        tracing::warn!(
            endpoint_id = %viewer.endpoint,
            write_timeout = ?viewer.write_timeout,
            "evicted a viewer that stopped reading"
        );
    }
}
