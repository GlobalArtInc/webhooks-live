#![allow(dead_code)]

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::stream;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

pub const ENDPOINT: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";
pub const WAIT: Duration = Duration::from_secs(20);

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "whl-cli-it-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    pub fn state_file(&self) -> PathBuf {
        self.0.join("state.json")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn event_frame(name: &str, id: Option<u64>, data: &str) -> Bytes {
    let mut frame = String::new();
    frame.push_str("event: ");
    frame.push_str(name);
    frame.push('\n');
    if let Some(id) = id {
        frame.push_str("id: ");
        frame.push_str(&id.to_string());
        frame.push('\n');
    }
    for line in data.split(['\n', '\r']) {
        frame.push_str("data: ");
        frame.push_str(line);
        frame.push('\n');
    }
    frame.push('\n');
    Bytes::from(frame)
}

#[derive(Clone)]
pub struct CaptureSpec {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl CaptureSpec {
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_owned(),
            path: path.to_owned(),
            query: String::new(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn query(mut self, query: &str) -> Self {
        self.query = query.to_owned();
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.body = body.to_vec();
        self
    }
}

struct Stored {
    seq: u64,
    spec: CaptureSpec,
}

impl Stored {
    fn id(&self) -> String {
        format!("00000000-0000-7000-8000-{:012}", self.seq)
    }

    fn summary(&self) -> Value {
        json!({
            "id": self.id(),
            "seq": self.seq,
            "endpoint_id": ENDPOINT,
            "method": self.spec.method,
            "path": self.spec.path,
            "query_string": self.spec.query,
            "client_ip": "203.0.113.7",
            "content_type": null,
            "body_size": self.spec.body.len(),
            "status_override": null,
            "response_status": 200,
            "forward_state": null,
        })
    }

    fn detail(&self) -> Value {
        let mut value = self.summary();
        value["host"] = json!("webhooks.live");
        value["url"] = json!(format!(
            "https://webhooks.live/{ENDPOINT}{}",
            self.spec.path
        ));
        value["headers"] = self
            .spec
            .headers
            .iter()
            .map(|(name, value)| json!({"name": name, "value": value}))
            .collect();
        value["body"] = json!({"encoding": "base64", "data": "", "truncated": true});
        value
    }
}

enum Message {
    Data(Bytes),
    Close,
}

#[derive(Default)]
pub struct FakeOptions {
    pub token: Option<&'static str>,
    pub page_size: Option<usize>,
}

struct Shared {
    captures: Mutex<Vec<Stored>>,
    listeners: Mutex<Vec<mpsc::UnboundedSender<Message>>>,
    token: Option<&'static str>,
    page_size: usize,
    gone: AtomicBool,
    authorizations: Mutex<Vec<Option<String>>>,
    last_event_ids: Mutex<Vec<Option<String>>>,
    requests: Mutex<Vec<String>>,
}

pub struct Fake {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

fn error_response(status: StatusCode, code: &str) -> Response {
    let body = json!({"error": {"code": code, "message": code}});
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

fn guard(shared: &Shared, headers: &HeaderMap, uri: &Uri) -> Option<Response> {
    shared
        .requests
        .lock()
        .expect("lock")
        .push(uri.path().to_owned());
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    shared
        .authorizations
        .lock()
        .expect("lock")
        .push(authorization.clone());
    if shared.gone.load(Ordering::SeqCst) {
        return Some(error_response(StatusCode::GONE, "endpoint_gone"));
    }
    if let Some(token) = shared.token
        && authorization.as_deref() != Some(&format!("Bearer {token}"))
    {
        return Some(error_response(StatusCode::UNAUTHORIZED, "invalid_token"));
    }
    None
}

async fn stream_route(
    State(shared): State<Arc<Shared>>,
    Path(_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(refusal) = guard(&shared, &headers, &uri) {
        return refusal;
    }
    let last = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    shared
        .last_event_ids
        .lock()
        .expect("lock")
        .push(last.clone());
    let (sender, receiver) = mpsc::unbounded_channel();
    {
        let captures = shared.captures.lock().expect("lock");
        let last_seq = captures.last().map_or(0, |stored| stored.seq);
        let ready =
            json!({"endpoint_id": ENDPOINT, "last_seq": last_seq, "capture_count": captures.len()});
        let _ = sender.send(Message::Data(event_frame(
            "ready",
            None,
            &ready.to_string(),
        )));
        if let Some(from) = last.and_then(|text| text.parse::<u64>().ok()) {
            for stored in captures.iter().filter(|stored| stored.seq > from).take(100) {
                let frame = event_frame("capture", Some(stored.seq), &stored.summary().to_string());
                let _ = sender.send(Message::Data(frame));
            }
        }
        shared.listeners.lock().expect("lock").push(sender);
    }
    let body = Body::from_stream(stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Some(Message::Data(bytes)) => Some((Ok::<Bytes, Infallible>(bytes), receiver)),
            Some(Message::Close) | None => None,
        }
    }));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .expect("stream response")
}

async fn list_route(
    State(shared): State<Arc<Shared>>,
    Path(_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(refusal) = guard(&shared, &headers, &uri) {
        return refusal;
    }
    let bound = |name: &str| query.get(name).and_then(|text| text.parse::<u64>().ok());
    let after = bound("after_seq")
        .unwrap_or(0)
        .max(bound("cursor").unwrap_or(0));
    let captures = shared.captures.lock().expect("lock");
    let remaining: Vec<&Stored> = captures
        .iter()
        .filter(|stored| stored.seq > after)
        .collect();
    let page: Vec<&Stored> = remaining.iter().copied().take(shared.page_size).collect();
    let next_cursor = (remaining.len() > page.len())
        .then(|| page.last().map(|stored| stored.seq.to_string()))
        .flatten();
    let items: Vec<Value> = page.iter().map(|stored| stored.summary()).collect();
    axum::Json(json!({"items": items, "next_cursor": next_cursor, "total": captures.len()}))
        .into_response()
}

async fn detail_route(
    State(shared): State<Arc<Shared>>,
    Path((_id, capture_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(refusal) = guard(&shared, &headers, &uri) {
        return refusal;
    }
    let captures = shared.captures.lock().expect("lock");
    match captures.iter().find(|stored| stored.id() == capture_id) {
        Some(stored) => axum::Json(stored.detail()).into_response(),
        None => error_response(StatusCode::NOT_FOUND, "capture_not_found"),
    }
}

async fn body_route(
    State(shared): State<Arc<Shared>>,
    Path((_id, capture_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(refusal) = guard(&shared, &headers, &uri) {
        return refusal;
    }
    let captures = shared.captures.lock().expect("lock");
    match captures.iter().find(|stored| stored.id() == capture_id) {
        Some(stored) => (
            [(header::CONTENT_TYPE, "application/octet-stream")],
            stored.spec.body.clone(),
        )
            .into_response(),
        None => error_response(StatusCode::NOT_FOUND, "capture_not_found"),
    }
}

impl Fake {
    pub async fn start(options: FakeOptions) -> Self {
        let shared = Arc::new(Shared {
            captures: Mutex::new(Vec::new()),
            listeners: Mutex::new(Vec::new()),
            token: options.token,
            page_size: options.page_size.unwrap_or(200),
            gone: AtomicBool::new(false),
            authorizations: Mutex::new(Vec::new()),
            last_event_ids: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/api/v1/endpoints/{id}/stream", get(stream_route))
            .route("/api/v1/endpoints/{id}/captures", get(list_route))
            .route(
                "/api/v1/endpoints/{id}/captures/{capture_id}",
                get(detail_route),
            )
            .route(
                "/api/v1/endpoints/{id}/captures/{capture_id}/body",
                get(body_route),
            )
            .with_state(shared.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { addr, shared }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn push(&self, spec: CaptureSpec) -> u64 {
        let mut captures = self.shared.captures.lock().expect("lock");
        let seq = captures.last().map_or(0, |stored| stored.seq) + 1;
        let stored = Stored { seq, spec };
        let frame = event_frame("capture", Some(seq), &stored.summary().to_string());
        captures.push(stored);
        let mut listeners = self.shared.listeners.lock().expect("lock");
        listeners.retain(|listener| listener.send(Message::Data(frame.clone())).is_ok());
        seq
    }

    pub fn drop_streams(&self) {
        let mut listeners = self.shared.listeners.lock().expect("lock");
        for listener in listeners.drain(..) {
            let _ = listener.send(Message::Close);
        }
    }

    pub fn set_gone(&self) {
        self.shared.gone.store(true, Ordering::SeqCst);
    }

    pub fn last_event_ids(&self) -> Vec<Option<String>> {
        self.shared.last_event_ids.lock().expect("lock").clone()
    }

    pub fn authorizations(&self) -> Vec<Option<String>> {
        self.shared.authorizations.lock().expect("lock").clone()
    }

    pub fn requests(&self) -> Vec<String> {
        self.shared.requests.lock().expect("lock").clone()
    }
}

#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_slice())
    }

    pub fn header_values(&self, name: &str) -> Vec<&[u8]> {
        self.headers
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_slice())
            .collect()
    }
}

pub struct Target {
    pub addr: SocketAddr,
    received: Arc<Mutex<Vec<Received>>>,
}

impl Target {
    pub async fn start() -> Self {
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = received.clone();
        let app = Router::new().fallback(move |request: Request| {
            let log = log.clone();
            async move {
                let (parts, body) = request.into_parts();
                let bytes = axum::body::to_bytes(body, usize::MAX)
                    .await
                    .unwrap_or_default();
                log.lock().expect("lock").push(Received {
                    method: parts.method.to_string(),
                    target: parts
                        .uri
                        .path_and_query()
                        .map(|value| value.as_str().to_owned())
                        .unwrap_or_default(),
                    headers: parts
                        .headers
                        .iter()
                        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
                        .collect(),
                    body: bytes.to_vec(),
                });
                StatusCode::ACCEPTED
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { addr, received }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().expect("lock").clone()
    }
}

pub async fn closed_port() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    listener.local_addr().expect("address")
}

pub struct Cli {
    child: Child,
    lines: mpsc::UnboundedReceiver<String>,
    pub transcript: Vec<String>,
}

impl Cli {
    pub fn spawn(home: &Scratch, envs: &[(&str, &str)], args: &[String]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_webhooks-live-cli"));
        command
            .env_clear()
            .env("HOME", &home.0)
            .envs(envs.iter().copied())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("spawn the CLI");
        let (sender, lines) = mpsc::unbounded_channel();
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");
        for (prefix, reader) in [
            (
                "out",
                Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
            ),
            ("err", Box::new(stderr)),
        ] {
            let sender = sender.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if sender.send(format!("{prefix}: {line}")).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            child,
            lines,
            transcript: Vec::new(),
        }
    }

    pub async fn wait_for(&mut self, needle: &str) -> String {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            match tokio::time::timeout_at(deadline, self.lines.recv()).await {
                Ok(Some(line)) => {
                    self.transcript.push(line.clone());
                    if line.contains(needle) {
                        return line;
                    }
                }
                Ok(None) => panic!(
                    "the CLI closed its output before {needle:?}: {:?}",
                    self.transcript
                ),
                Err(_) => panic!("timed out waiting for {needle:?}: {:?}", self.transcript),
            }
        }
    }

    pub async fn kill(&mut self) {
        self.child.kill().await.expect("kill the CLI");
    }

    pub fn interrupt(&self) {
        let pid = self.child.id().expect("running child");
        let status = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .expect("run kill");
        assert!(status.success());
    }

    pub async fn exit_code(&mut self) -> Option<i32> {
        tokio::time::timeout(WAIT, self.child.wait())
            .await
            .expect("the CLI exits in time")
            .expect("wait for the CLI")
            .code()
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

pub async fn eventually(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition not met in time"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
