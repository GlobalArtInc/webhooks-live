#![allow(dead_code)]

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::Request;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use sqlx::PgPool;
use sqlx::types::Uuid;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tower::ServiceExt;
use whl_core::endpoint_id::EndpointId;
use whl_live::{
    Event, Hub, ListenerHandle, ListenerSettings, LiveState, Notification, StreamAuthorizer,
    StreamSettings, router, spawn_listener_with,
};
use whl_store::Store;
use whl_testkit::TestDb;

pub const WAIT: Duration = Duration::from_secs(30);
pub const POLL_INTERVAL: Duration = Duration::from_millis(20);
pub const RETENTION: Duration = Duration::from_secs(3600);
pub const HUB_TEST_CAPACITY: usize = 256;

pub struct TestApp {
    pub addr: SocketAddr,
    pub hub: Arc<Hub>,
    pub live: LiveState,
    pub listener: ListenerHandle,
    stop: watch::Sender<bool>,
    server: JoinHandle<()>,
}

impl TestApp {
    pub async fn start(db: &TestDb) -> Self {
        Self::start_with(db, HUB_TEST_CAPACITY, StreamSettings::new(RETENTION)).await
    }

    pub async fn start_with(db: &TestDb, capacity: usize, settings: StreamSettings) -> Self {
        let store = Store::from_pool(db.pool().clone());
        Self::start_on(store, capacity, settings, ListenerSettings::default()).await
    }

    pub async fn start_authorized(
        db: &TestDb,
        settings: StreamSettings,
        authorizer: Arc<dyn StreamAuthorizer>,
    ) -> Self {
        let store = Store::from_pool(db.pool().clone());
        Self::start_inner(
            store,
            HUB_TEST_CAPACITY,
            settings,
            ListenerSettings::default(),
            Some(authorizer),
        )
        .await
    }

    pub async fn start_on(
        store: Store,
        capacity: usize,
        settings: StreamSettings,
        listening: ListenerSettings,
    ) -> Self {
        Self::start_inner(store, capacity, settings, listening, None).await
    }

    async fn start_inner(
        store: Store,
        capacity: usize,
        settings: StreamSettings,
        listening: ListenerSettings,
        authorizer: Option<Arc<dyn StreamAuthorizer>>,
    ) -> Self {
        let hub = Hub::new(capacity);
        let live = LiveState::new(store.clone(), Arc::clone(&hub), settings);
        let live = match authorizer {
            Some(authorizer) => live.with_authorizer(authorizer),
            None => live,
        };
        let (stop, mut stopped) = watch::channel(false);
        let listener = spawn_listener_with(store, Arc::clone(&hub), listening, async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        });
        listener
            .wait_listening()
            .await
            .expect("the listener starts listening");
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral port binds");
        let addr = socket.local_addr().expect("the socket has an address");
        let app = router(live.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(socket, app).into_future().await;
        });
        Self {
            addr,
            hub,
            live,
            listener,
            stop,
            server,
        }
    }

    pub async fn open(&self, endpoint: &EndpointId) -> SseClient {
        SseClient::open(self.addr, &stream_path(endpoint), &[]).await
    }

    pub async fn open_with(&self, path: &str, headers: &[(&str, &str)]) -> SseClient {
        SseClient::open(self.addr, path, headers).await
    }

    pub async fn open_over(&self, io: impl Pipe, endpoint: &EndpointId) -> SseClient {
        SseClient::open_on(io, &stream_path(endpoint), &[]).await
    }
}

pub const PIPE_BYTES: usize = 1024;
pub const PIPE_WRITE_BUFFER: usize = 16 * 1024;

pub trait Pipe: AsyncRead + AsyncWrite + Unpin + Send + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Pipe for T {}

pub fn pipe() -> (tokio::io::DuplexStream, tokio::io::DuplexStream) {
    tokio::io::duplex(PIPE_BYTES)
}

pub async fn serve_http1(io: impl Pipe, live: LiveState) -> Result<(), hyper::Error> {
    serve_http1_until(io, live, std::future::pending()).await
}

pub async fn serve_http1_until(
    io: impl Pipe,
    live: LiveState,
    stop: impl std::future::Future<Output = ()>,
) -> Result<(), hyper::Error> {
    let app = router(live);
    let service = service_fn(move |request: Request<Incoming>| app.clone().oneshot(request));
    let mut connection = std::pin::pin!(
        http1::Builder::new()
            .max_buf_size(PIPE_WRITE_BUFFER)
            .serve_connection(TokioIo::new(io), service)
    );
    tokio::select! {
        result = connection.as_mut() => result,
        () = stop => {
            connection.as_mut().graceful_shutdown();
            connection.await
        }
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        self.server.abort();
    }
}

pub fn stream_path(endpoint: &EndpointId) -> String {
    format!("/api/v1/endpoints/{endpoint}/stream")
}

pub fn new_endpoint_id() -> EndpointId {
    EndpointId::generate().expect("the operating system provides entropy")
}

pub async fn insert_endpoint(pool: &PgPool) -> EndpointId {
    let id = new_endpoint_id();
    sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
        .bind(id.unpack())
        .execute(pool)
        .await
        .expect("the endpoint inserts");
    id
}

pub async fn insert_captures(pool: &PgPool, endpoint: &EndpointId, first: i64, last: i64) {
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, content_type, body_size, stored_bytes, response_status) \
         SELECT $1, s, gen_random_uuid(), now(), 'POST', 'http://h/' || $1 || '/p' || s || '?a=1', \
         '/p' || s, 'h', '10.0.0.1', '[]', '[]', 'text/plain', 3, 3, 200 \
         FROM generate_series($2::bigint, $3::bigint) AS s",
    )
    .bind(endpoint.unpack())
    .bind(first)
    .bind(last)
    .execute(pool)
    .await
    .expect("the captures insert");
    sqlx::query(
        "UPDATE endpoints SET capture_count = capture_count + $2, last_seq = $3 WHERE id = $1",
    )
    .bind(endpoint.unpack())
    .bind(i32::try_from(last - first + 1).expect("the count fits"))
    .bind(last)
    .execute(pool)
    .await
    .expect("the counters update");
}

pub async fn insert_forward_job(pool: &PgPool, endpoint: &EndpointId, seq: i64) -> (Uuid, Uuid) {
    let job: Uuid = sqlx::query_scalar(
        "INSERT INTO forward_jobs (id, capture_id, kind, target_url, method, header_allowlist, state) \
         SELECT gen_random_uuid(), id, 'manual', 'http://target.test', 'POST', '{}', 'failed' \
         FROM captures WHERE endpoint_id = $1 AND seq = $2 RETURNING id",
    )
    .bind(endpoint.unpack())
    .bind(seq)
    .fetch_one(pool)
    .await
    .expect("the job inserts");
    sqlx::query(
        "INSERT INTO forward_attempts (job_id, attempt, started_at, duration_ms, status, response_content_type, response_body) \
         VALUES ($1, 1, now(), 9, 502, 'text/plain', 'bad gateway'::bytea)",
    )
    .bind(job)
    .execute(pool)
    .await
    .expect("the attempt inserts");
    let capture: Uuid = sqlx::query_scalar("SELECT capture_id FROM forward_jobs WHERE id = $1")
        .bind(job)
        .fetch_one(pool)
        .await
        .expect("the capture id is read");
    (capture, job)
}

pub fn padded_event(endpoint: &EndpointId, seq: i64, padding: usize) -> Event {
    match Notification::decode(&padded_capture_payload(endpoint, seq, padding))
        .expect("the payload decodes")
    {
        Notification::Event { event, .. } => event,
        Notification::Heartbeat => panic!("a capture payload is not a heartbeat"),
    }
}

pub async fn notify(pool: &PgPool, payload: &str) {
    sqlx::query("SELECT pg_notify('whl_events', $1)")
        .bind(payload)
        .execute(pool)
        .await
        .expect("the notification is sent");
}

pub fn capture_payload(endpoint: &EndpointId, seq: i64) -> String {
    format!(
        r#"{{"t":"capture","e":"{endpoint}","seq":{seq},"s":{{"id":"00000000-0000-7000-8000-{seq:012}","seq":{seq},"endpoint_id":"{endpoint}","method":"POST","path":"/live{seq}"}}}}"#
    )
}

pub fn padded_capture_payload(endpoint: &EndpointId, seq: i64, padding: usize) -> String {
    let pad = "x".repeat(padding);
    format!(r#"{{"t":"capture","e":"{endpoint}","seq":{seq},"s":{{"seq":{seq},"pad":"{pad}"}}}}"#)
}

pub async fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

pub async fn terminate_listener_backend(pool: &PgPool) -> u64 {
    sqlx::query(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE datname = current_database() AND query LIKE 'LISTEN%' AND pid <> pg_backend_pid()",
    )
    .execute(pool)
    .await
    .expect("the backend terminates")
    .rows_affected()
}

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub event: Option<String>,
    pub id: Option<String>,
    pub data: String,
    pub retry: Option<u64>,
    pub comment: Option<String>,
}

impl Frame {
    pub fn is_event(&self, name: &str) -> bool {
        self.event.as_deref() == Some(name)
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.data).expect("the data is JSON")
    }
}

pub struct SseClient {
    reader: BufReader<Box<dyn Pipe>>,
    buffer: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    chunked: bool,
}

impl SseClient {
    pub async fn open(addr: SocketAddr, path: &str, extra: &[(&str, &str)]) -> Self {
        let stream = TcpStream::connect(addr).await.expect("the server accepts");
        Self::open_on(stream, path, extra).await
    }

    pub async fn open_on(mut stream: impl Pipe, path: &str, extra: &[(&str, &str)]) -> Self {
        let mut request =
            format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n");
        for (name, value) in extra {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("the request is written");
        let boxed: Box<dyn Pipe> = Box::new(stream);
        let mut reader = BufReader::new(boxed);
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .await
            .expect("the status line arrives");
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("the status line has a code");
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("a header arrives");
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let (name, value) = line.split_once(':').expect("a header has a colon");
            headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
        }
        let chunked = headers
            .iter()
            .any(|(name, value)| name == "transfer-encoding" && value == "chunked");
        Self {
            reader,
            buffer: String::new(),
            status,
            headers,
            chunked,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }

    pub async fn read_body(mut self) -> String {
        let length: usize = self
            .header("content-length")
            .and_then(|value| value.parse().ok())
            .expect("the response has a content length");
        let mut body = vec![0_u8; length];
        self.reader
            .read_exact(&mut body)
            .await
            .expect("the body arrives");
        String::from_utf8_lossy(&body).into_owned()
    }

    async fn fill(&mut self) -> bool {
        if !self.chunked {
            return false;
        }
        let mut size_line = String::new();
        match self.reader.read_line(&mut size_line).await {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        let Ok(size) = usize::from_str_radix(size_line.trim(), 16) else {
            return false;
        };
        if size == 0 {
            return false;
        }
        let mut chunk = vec![0_u8; size + 2];
        if self.reader.read_exact(&mut chunk).await.is_err() {
            return false;
        }
        self.buffer
            .push_str(&String::from_utf8_lossy(&chunk[..size]));
        true
    }

    pub async fn next_frame(&mut self) -> Option<Frame> {
        loop {
            if let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                return Some(parse_frame(&block));
            }
            if !self.fill().await {
                return None;
            }
        }
    }

    pub async fn next_event(&mut self) -> Option<Frame> {
        loop {
            let frame = tokio::time::timeout(WAIT, self.next_frame())
                .await
                .expect("a frame arrives in time")?;
            if frame.event.is_some() {
                return Some(frame);
            }
        }
    }

    pub async fn expect_event(&mut self, name: &str) -> Frame {
        let frame = self
            .next_event()
            .await
            .unwrap_or_else(|| panic!("the stream ended while waiting for {name}"));
        assert!(frame.is_event(name), "expected {name}, got {frame:?}");
        frame
    }

    pub async fn expect_capture(&mut self, seq: i64) -> Frame {
        let frame = self.expect_event("capture").await;
        assert_eq!(
            frame.id.as_deref(),
            Some(seq.to_string().as_str()),
            "{frame:?}"
        );
        frame
    }

    pub async fn expect_end(&mut self) {
        let ended =
            tokio::time::timeout(WAIT, async { while self.next_frame().await.is_some() {} }).await;
        assert!(ended.is_ok(), "the stream did not end in time");
    }
}

fn parse_frame(block: &str) -> Frame {
    let mut frame = Frame::default();
    let mut data: Vec<&str> = Vec::new();
    for line in block.lines() {
        if let Some(comment) = line.strip_prefix(':') {
            frame.comment = Some(comment.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("event:") {
            frame.event = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("id:") {
            frame.id = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("retry:") {
            frame.retry = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    frame.data = data.join("\n");
    frame
}
