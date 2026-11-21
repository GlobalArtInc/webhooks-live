use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tokio::task::JoinHandle;
use whl_api::{
    ApiSettings, ApiState, ComposerLimits, ComposerSettings, ForwardSettings, LocalDelivery,
    LocalFuture, LocalReply, LocalRequest,
};
use whl_core::capture_id::CaptureId;
use whl_forward::{GuardConfig, StaticLookup};
use whl_store::Store;
use whl_testkit::TestDb;

use super::{Harness, Reply, api_settings, send};

pub const PEER: &str = "203.0.113.7";

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: Method,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

pub struct Target {
    pub addr: SocketAddr,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    task: JoinHandle<()>,
}

impl Target {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log is not poisoned").clone()
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn target_handler(State(seen): State<Arc<Mutex<Vec<Seen>>>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, 8 * 1024 * 1024)
        .await
        .expect("the request body is readable");
    let path = parts.uri.path().to_owned();
    seen.lock().expect("the log is not poisoned").push(Seen {
        method: parts.method,
        path_and_query: parts
            .uri
            .path_and_query()
            .map(ToString::to_string)
            .unwrap_or_default(),
        headers: parts.headers,
        body: bytes.to_vec(),
    });
    match path.as_str() {
        "/big" => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/octet-stream")],
            vec![b'x'; 2 * 1024 * 1024],
        )
            .into_response(),
        "/redirect-private" => (
            StatusCode::FOUND,
            [(header::LOCATION, "http://169.254.169.254/latest")],
        )
            .into_response(),
        "/redirect-ok" => (StatusCode::FOUND, [(header::LOCATION, "/ok")]).into_response(),
        _ => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain")],
            "pong",
        )
            .into_response(),
    }
}

pub async fn target() -> Target {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a loopback port is free");
    let addr = listener.local_addr().expect("the listener has an address");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .fallback(target_handler)
        .with_state(Arc::clone(&seen));
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Target { addr, seen, task }
}

#[derive(Debug, Clone)]
pub struct Delivered {
    pub method: Method,
    pub host: String,
    pub path_and_query: String,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub client_ip: IpAddr,
}

pub struct MockLocal {
    delivered: Mutex<Vec<Delivered>>,
    delay: Duration,
}

impl MockLocal {
    pub fn new() -> Arc<Self> {
        Self::slow(Duration::ZERO)
    }

    pub fn slow(delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            delivered: Mutex::new(Vec::new()),
            delay,
        })
    }

    pub fn delivered(&self) -> Vec<Delivered> {
        self.delivered
            .lock()
            .expect("the log is not poisoned")
            .clone()
    }
}

impl LocalDelivery for MockLocal {
    fn deliver(&self, request: LocalRequest) -> LocalFuture<'_> {
        Box::pin(async move {
            self.delivered
                .lock()
                .expect("the log is not poisoned")
                .push(Delivered {
                    method: request.method,
                    host: request.host,
                    path_and_query: request.path_and_query,
                    headers: request.headers,
                    body: request.body,
                    client_ip: request.client_ip,
                });
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let id = CaptureId::generate().expect("an id is generated");
            let mut headers = HeaderMap::new();
            headers.insert(
                "x-request-id",
                id.to_string().parse().expect("a header value"),
            );
            headers.insert(header::CONTENT_TYPE, "text/plain".parse().expect("a value"));
            Ok(LocalReply {
                status: StatusCode::OK,
                headers,
                body: Bytes::from_static(b"stored"),
            })
        })
    }
}

pub fn limits(endpoint_per_hour: u32, network_per_day: u32, user_per_hour: u32) -> ComposerLimits {
    ComposerLimits {
        endpoint_per_hour,
        network_per_day,
        user_per_hour,
    }
}

pub fn composer(local: &Arc<MockLocal>, limits: ComposerLimits) -> ComposerSettings {
    ComposerSettings::new(true, limits, 64)
        .with_allowed_networks(vec!["127.0.0.1/32".parse().expect("a CIDR")])
        .with_local(Arc::clone(local) as Arc<dyn LocalDelivery>)
}

pub fn lookup() -> StaticLookup {
    StaticLookup::new()
        .with_answer(
            "public.test",
            &["93.184.216.34".parse().expect("an address")],
        )
        .with_answer("private.test", &["10.0.0.5".parse().expect("an address")])
}

pub async fn open_harness(composer: ComposerSettings) -> Harness {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let settings: ApiSettings = api_settings();
    let forward = ForwardSettings::new(GuardConfig::default(), Arc::new(lookup()));
    let state = ApiState::new(store, settings)
        .with_forward(forward)
        .with_composer(composer);
    Harness {
        db,
        app: whl_api::router(state),
    }
}

pub fn send_request(peer: &str, method: Method, uri: &str, body: &str) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("the request is valid");
    let address = SocketAddr::new(peer.parse().expect("a peer address"), 40000);
    request.extensions_mut().insert(ConnectInfo(address));
    request
}

pub async fn post_send(app: &Router, peer: &str, body: &Value) -> Reply {
    send(
        app,
        send_request(peer, Method::POST, "/api/v1/send", &body.to_string()),
    )
    .await
}

pub fn simple(url: &str) -> Value {
    json!({"method": "GET", "url": url})
}

pub fn error_code(reply: &Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

pub fn field_code(reply: &Reply) -> String {
    reply.json()["error"]["fields"][0]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Captured {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("the buffer is readable")).into_owned()
    }
}

static CAPTURED: OnceLock<Captured> = OnceLock::new();

pub fn captured_logs() -> &'static Captured {
    CAPTURED.get_or_init(|| {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .with_writer(captured.clone())
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        captured
    })
}
