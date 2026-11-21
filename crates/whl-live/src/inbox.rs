use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Extension;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;
use futures_util::stream::{SelectAll, unfold};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::time::Instant;
use whl_core::endpoint_id::EndpointId;
use whl_store::merge::merge_lanes;
use whl_store::stream::{CaptureSummary, EndpointLiveState};
use whl_store::{Order, Store};

use crate::authorize::{StreamRequest, StreamVerdict};
use crate::emit::{self, ConnectionGauge};
use crate::event::{Event, Outcome, ResyncReason};
use crate::hub::Subscription;
use crate::route::{
    Ending, FrameStream, LAST_EVENT_ID_HEADER, LAST_EVENT_ID_PARAM, LIFETIME_JITTER, LiveState,
    OUTBOX_FRAMES, RETRY_MILLIS, error_response, flush_frames, gone, stream_response, summary_json,
    unavailable,
};
use crate::slots::SlotGuard;
use crate::vector::SeqVector;
use crate::{jitter, load, sse};

pub const INBOX_STREAM_PATH: &str = "/api/v1/inbox/stream";
pub const INBOX_MAX_ENDPOINTS: usize = 50;
pub const INBOX_MAX_ANONYMOUS_ENDPOINTS: usize = 20;

#[derive(Debug, Clone)]
pub struct InboxSelection {
    pub endpoints: Vec<EndpointId>,
    pub user: Option<String>,
    pub address: Option<IpAddr>,
}

pub type SelectionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<InboxSelection, Response>> + Send + 'a>>;

pub trait InboxSelector: fmt::Debug + Send + Sync + 'static {
    fn select<'a>(&'a self, request: &'a StreamRequest, uri: &'a Uri) -> SelectionFuture<'a>;
}

type LaneItem = (usize, Result<Arc<Event>, RecvError>);
type LaneStream = Pin<Box<dyn Stream<Item = LaneItem> + Send>>;

#[derive(Serialize)]
struct ReadyEndpoint<'a> {
    endpoint_id: &'a str,
    last_seq: i64,
    capture_count: i32,
}

#[derive(Serialize)]
struct ReadyBody<'a> {
    endpoints: Vec<ReadyEndpoint<'a>>,
}

fn lane(index: usize, subscription: Subscription) -> LaneStream {
    Box::pin(unfold(Some(subscription), move |state| async move {
        let mut subscription = state?;
        let received = subscription.recv().await;
        let next = if matches!(received, Err(RecvError::Closed)) {
            None
        } else {
            Some(subscription)
        };
        Some(((index, received), next))
    }))
}

fn resume_vector(headers: &HeaderMap, query: Option<&str>) -> SeqVector {
    let mut resume = SeqVector::new();
    let from_header = headers
        .get(LAST_EVENT_ID_HEADER)
        .and_then(|value| value.to_str().ok());
    let from_query = query.and_then(|query| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(name, _)| *name == LAST_EVENT_ID_PARAM)
            .map(|(_, value)| value)
    });
    for (source, text) in [
        ("Last-Event-ID header", from_header),
        ("last_event_id parameter", from_query),
    ] {
        let Some(text) = text else {
            continue;
        };
        match SeqVector::decode(text) {
            Ok(vector) => resume.merge_max(&vector),
            Err(error) => {
                tracing::info!(source, %error, "ignored a malformed inbox resume vector");
            }
        }
    }
    resume
}

fn too_many_streams() -> Response {
    let mut response = error_response(
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "too many inbox streams are open",
    );
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("30"));
    response
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn distinct(endpoints: Vec<EndpointId>) -> Vec<EndpointId> {
    let mut seen = HashSet::new();
    endpoints
        .into_iter()
        .filter(|endpoint| seen.insert(endpoint.clone()))
        .collect()
}

pub(crate) async fn stream_inbox(
    State(state): State<LiveState>,
    connect: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(selector) = state.inbox.clone() else {
        return crate::route::not_found();
    };
    let resume = resume_vector(&headers, uri.query());
    let peer = connect.map(|Extension(ConnectInfo(peer))| peer.ip());
    let request = StreamRequest::new(headers, peer);
    let selection = match selector.select(&request, &uri).await {
        Ok(selection) => selection,
        Err(refusal) => return no_store(refusal),
    };
    let endpoints = distinct(selection.endpoints);
    if endpoints.len() > INBOX_MAX_ENDPOINTS {
        tracing::error!(
            count = endpoints.len(),
            "an inbox selector returned more endpoints than the bound"
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "unexpected server failure",
        );
    }

    let Some(slot) = state.slots.acquire(
        selection.user.as_deref(),
        selection.address,
        state.settings.inbox_streams_per_principal,
    ) else {
        return too_many_streams();
    };

    let subscriptions: Vec<Subscription> = endpoints
        .iter()
        .map(|endpoint| state.hub.subscribe(endpoint))
        .collect();
    let closing = subscriptions
        .iter()
        .any(Subscription::was_closing_at_subscribe);
    let mut lives = Vec::with_capacity(endpoints.len());
    if !closing && !endpoints.is_empty() {
        let mut states = match state
            .store
            .extend_endpoints_lifetime(&endpoints, state.settings.retention)
            .await
        {
            Ok(states) => states,
            Err(error) => {
                tracing::error!(%error, "cannot read the endpoints for an inbox stream");
                return unavailable();
            }
        };
        for endpoint in &endpoints {
            match states.remove(endpoint.unpack()) {
                Some(live) => lives.push(live),
                None => return gone(),
            }
        }
    }

    let mut viewer = InboxViewer::new(endpoints, subscriptions, request, slot, &state);
    viewer.queue.push_back(sse::retry_frame(RETRY_MILLIS));
    if closing {
        viewer.push_plain("bye", "{}");
        viewer.finishing = true;
    } else {
        let targets = viewer.start(&resume, &lives);
        viewer
            .replay(&state.store, targets, state.settings.replay_limit)
            .await;
    }

    let (outbox, frames) = mpsc::channel(OUTBOX_FRAMES);
    tokio::spawn(pump(viewer, outbox, state));
    stream_response(Body::from_stream(FrameStream { frames }))
}

struct InboxViewer {
    endpoints: Vec<EndpointId>,
    lane_of: HashMap<String, usize>,
    lanes: SelectAll<LaneStream>,
    positions: SeqVector,
    last_seqs: Vec<i64>,
    queue: VecDeque<Bytes>,
    request: StreamRequest,
    ping_interval: Duration,
    recheck_interval: Duration,
    write_timeout: Duration,
    next_ping: Instant,
    next_recheck: Instant,
    deadline: Instant,
    last_resync: Option<Arc<Event>>,
    resync_queued: bool,
    finishing: bool,
    _slot: SlotGuard,
    _connection: ConnectionGauge,
}

impl InboxViewer {
    fn new(
        endpoints: Vec<EndpointId>,
        subscriptions: Vec<Subscription>,
        request: StreamRequest,
        slot: SlotGuard,
        state: &LiveState,
    ) -> Self {
        let settings = &state.settings;
        let now = Instant::now();
        let lane_of = endpoints
            .iter()
            .enumerate()
            .map(|(index, endpoint)| (endpoint.unpack().to_owned(), index))
            .collect();
        let mut lanes = SelectAll::new();
        for (index, subscription) in subscriptions.into_iter().enumerate() {
            lanes.push(lane(index, subscription));
        }
        Self {
            endpoints,
            lane_of,
            lanes,
            positions: SeqVector::new(),
            last_seqs: Vec::new(),
            queue: VecDeque::new(),
            request,
            ping_interval: settings.ping_interval,
            recheck_interval: settings.recheck_interval,
            write_timeout: settings.write_timeout,
            next_ping: now + settings.ping_interval,
            next_recheck: now + settings.recheck_interval,
            deadline: now + jitter::jittered(settings.max_lifetime, LIFETIME_JITTER),
            last_resync: None,
            resync_queued: false,
            finishing: false,
            _slot: slot,
            _connection: ConnectionGauge::open(),
        }
    }

    fn push_frame(&mut self, name: &'static str, id: Option<&str>, data: &str) {
        self.queue
            .push_back(sse::event_frame_with_text_id(name, id, data));
        emit::event_sent(name);
    }

    fn push_plain(&mut self, name: &'static str, data: &str) {
        self.push_frame(name, None, data);
    }

    fn push_resync(&mut self, reason: ResyncReason) {
        if self.resync_queued {
            return;
        }
        self.resync_queued = true;
        emit::resync_sent(reason);
        let data = format!(r#"{{"reason":"{}"}}"#, reason.as_str());
        self.push_plain("resync", &data);
    }

    fn push_tagged(&mut self, name: &'static str, lane: usize, data: &str) {
        let tagged = match serde_json::from_str::<Value>(data) {
            Ok(Value::Object(mut object)) => {
                object.insert(
                    "endpoint_id".to_owned(),
                    Value::String(self.endpoints[lane].unpack().to_owned()),
                );
                serde_json::to_string(&object)
            }
            Ok(_) => {
                tracing::error!(event = name, "an inbox event payload is not an object");
                self.push_resync(ResyncReason::Gap);
                return;
            }
            Err(error) => Err(error),
        };
        match tagged {
            Ok(text) => self.push_plain(name, &text),
            Err(error) => {
                tracing::error!(%error, event = name, "cannot tag an inbox event with its endpoint");
                self.push_resync(ResyncReason::Gap);
            }
        }
    }

    fn start(&mut self, resume: &SeqVector, lives: &[EndpointLiveState]) -> Vec<usize> {
        let mut targets = Vec::new();
        let mut beyond = false;
        self.last_seqs = lives.iter().map(|live| live.last_seq).collect();
        for (index, live) in lives.iter().enumerate() {
            let position = match resume.get(&self.endpoints[index]) {
                Some(after) if after > live.last_seq => {
                    tracing::info!(endpoint_id = %self.endpoints[index], after, last_seq = live.last_seq, "an inbox resume point is beyond the endpoint's last seq");
                    beyond = true;
                    live.last_seq
                }
                Some(after) => {
                    if after < live.last_seq {
                        targets.push(index);
                    }
                    after
                }
                None => live.last_seq,
            };
            self.positions.set(&self.endpoints[index], position);
        }
        let ready = ReadyBody {
            endpoints: self
                .endpoints
                .iter()
                .zip(lives)
                .map(|(endpoint, live)| ReadyEndpoint {
                    endpoint_id: endpoint.unpack(),
                    last_seq: live.last_seq,
                    capture_count: live.capture_count,
                })
                .collect(),
        };
        match serde_json::to_string(&ready) {
            Ok(data) => {
                let id = self.positions.encode();
                self.push_frame("ready", Some(&id), &data);
            }
            Err(error) => tracing::error!(%error, "cannot encode the inbox ready event"),
        }
        if beyond {
            self.push_resync(ResyncReason::Gap);
        }
        targets
    }

    fn jump_to_last_seqs(&mut self) {
        for (index, last_seq) in self.last_seqs.clone().into_iter().enumerate() {
            self.positions.set(&self.endpoints[index], last_seq);
        }
    }

    async fn replay(&mut self, store: &Store, targets: Vec<usize>, limit: usize) {
        let mut lanes: Vec<Vec<CaptureSummary>> = Vec::with_capacity(targets.len());
        let mut total = 0_usize;
        for index in &targets {
            let remaining = limit.saturating_sub(total);
            let fetch = i64::try_from(remaining.saturating_add(1)).unwrap_or(i64::MAX);
            let after = self.positions.get(&self.endpoints[*index]).unwrap_or(0);
            match store
                .captures_after(&self.endpoints[*index], after, fetch)
                .await
            {
                Ok(captures) if captures.len() > remaining => {
                    self.jump_to_last_seqs();
                    self.push_resync(ResyncReason::Gap);
                    return;
                }
                Ok(captures) => {
                    total += captures.len();
                    lanes.push(captures);
                }
                Err(error) => {
                    tracing::error!(%error, endpoint_id = %self.endpoints[*index], "cannot replay captures for an inbox stream");
                    self.jump_to_last_seqs();
                    self.push_resync(ResyncReason::Gap);
                    return;
                }
            }
        }
        let merged = merge_lanes(&lanes, Order::Asc, total);
        for capture in &merged.items {
            self.push_capture(capture);
        }
    }

    fn push_capture(&mut self, capture: &CaptureSummary) {
        let Some(lane) = self.lane_of.get(&capture.endpoint_id).copied() else {
            tracing::error!(endpoint_id = %capture.endpoint_id, "a capture names an endpoint outside the inbox");
            return;
        };
        match summary_json(capture) {
            Ok(data) => self.deliver_capture(lane, capture.seq, &data),
            Err(error) => tracing::error!(%error, "cannot encode an inbox capture"),
        }
    }

    fn deliver_capture(&mut self, lane: usize, seq: i64, data: &str) {
        let endpoint = &self.endpoints[lane];
        if self.positions.get(endpoint).is_some_and(|held| seq <= held) {
            return;
        }
        self.positions.set(endpoint, seq);
        let id = self.positions.encode();
        self.push_frame("capture", Some(&id), data);
    }

    async fn accept(&mut self, lane: usize, event: &Arc<Event>, state: &LiveState) {
        match &**event {
            Event::Capture { seq, summary } => self.deliver_capture(lane, *seq, summary.unpack()),
            Event::CaptureDeleted { data } => {
                self.push_tagged("capture_deleted", lane, data.unpack());
            }
            Event::CapturesCleared => self.push_tagged("captures_cleared", lane, "{}"),
            Event::EndpointDeleted => self.push_tagged("endpoint_deleted", lane, "{}"),
            Event::EndpointUpdated { .. } => {}
            Event::ForwardResult {
                capture_id,
                job_id,
                lookup,
            } => {
                let loading =
                    load::forward_result(state, &self.endpoints[lane], capture_id, job_id);
                match lookup.resolve(loading).await {
                    Outcome::Data(data) => self.push_tagged("forward_result", lane, data.unpack()),
                    Outcome::Gone => {
                        tracing::debug!(endpoint_id = %self.endpoints[lane], "dropped a forward result whose subject no longer exists");
                    }
                    Outcome::Failed => self.push_resync(ResyncReason::Gap),
                }
            }
            Event::Resync { reason } => {
                let repeated = self
                    .last_resync
                    .as_ref()
                    .is_some_and(|previous| Arc::ptr_eq(previous, event));
                if !repeated {
                    self.last_resync = Some(Arc::clone(event));
                    self.push_resync(*reason);
                }
            }
            Event::AccessChanged => self.revalidate(state, Some(lane)).await,
            Event::Bye => {
                self.push_plain("bye", "{}");
                self.finishing = true;
            }
        }
    }

    async fn revalidate(&mut self, state: &LiveState, lane: Option<usize>) {
        self.next_recheck = Instant::now() + self.recheck_interval;
        let verdict = match lane {
            Some(lane) => {
                state
                    .authorizer
                    .still_allowed(&self.endpoints[lane], &self.request)
                    .await
            }
            None => {
                state
                    .authorizer
                    .still_allowed_all(&self.endpoints, &self.request)
                    .await
            }
        };
        match verdict {
            StreamVerdict::Allowed => {}
            StreamVerdict::Denied => {
                tracing::info!("closed an inbox stream whose viewer lost access");
                self.close_for_access();
            }
            StreamVerdict::Unavailable => {
                tracing::warn!("closed an inbox stream because its access could not be verified");
                self.close_for_access();
            }
        }
    }

    fn close_for_access(&mut self) {
        self.queue.clear();
        self.resync_queued = false;
        self.push_plain("bye", "{}");
        self.finishing = true;
    }

    async fn run(&mut self, outbox: &mpsc::Sender<Bytes>, state: &LiveState) -> Ending {
        let watching = !self.endpoints.is_empty();
        loop {
            if let Some(ending) = flush_frames(
                &mut self.queue,
                outbox,
                self.write_timeout,
                &mut self.next_ping,
                self.ping_interval,
            )
            .await
            {
                return ending;
            }
            self.resync_queued = false;
            if self.finishing || Instant::now() >= self.deadline {
                return Ending::Finished;
            }
            tokio::select! {
                received = self.lanes.next(), if watching => match received {
                    Some((lane, Ok(event))) => self.accept(lane, &event, state).await,
                    Some((_, Err(RecvError::Lagged(skipped)))) => {
                        tracing::debug!(skipped, "an inbox stream viewer lagged behind");
                        self.push_resync(ResyncReason::Lagged);
                    }
                    Some((_, Err(RecvError::Closed))) | None => return Ending::HubClosed,
                },
                () = tokio::time::sleep_until(self.next_ping) => {
                    self.queue.push_back(sse::ping_frame());
                }
                () = tokio::time::sleep_until(self.next_recheck) => self.revalidate(state, None).await,
                () = tokio::time::sleep_until(self.deadline) => return Ending::Finished,
                () = outbox.closed() => return Ending::Disconnected,
            }
        }
    }
}

async fn pump(mut viewer: InboxViewer, outbox: mpsc::Sender<Bytes>, state: LiveState) {
    if viewer.run(&outbox, &state).await == Ending::Stalled {
        tracing::warn!(
            endpoints = viewer.endpoints.len(),
            write_timeout = ?viewer.write_timeout,
            "evicted an inbox viewer that stopped reading"
        );
    }
}
