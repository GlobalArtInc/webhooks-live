#![allow(dead_code)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use url::Url;
use whl_core::capture_id::CaptureId;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_core::tier::{TierLimits, Tiers};
use whl_forward::GuardConfig;
use whl_notify::{
    NotifyHandle, NotifyLimits, NotifySettings, NotifyWorker, Transport, VendorHosts,
    channel_secret_name,
};
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::notify::{ChannelKind, CreateChannelOutcome, NewChannel, Trigger};
use whl_store::{EndpointSettings, Overflow, Store, Uuid};
use whl_testkit::TestDb;

pub const RETENTION: Duration = Duration::from_secs(3600);
pub const SLACK_URL: &str = "https://hooks.slack.com/services/T0/B0/slack-secret-part";
pub const DISCORD_URL: &str = "https://discord.com/api/webhooks/123456/discord-secret-part";
pub const TELEGRAM_TOKEN: &str = "123456:telegram-secret-part";
const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const VENDOR_HOSTS: [&str; 4] = [
    "hooks.slack.com",
    "discord.com",
    "discordapp.com",
    "api.telegram.org",
];

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

pub struct MockServer {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    task: JoinHandle<()>,
}

impl MockServer {
    pub async fn start(status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the mock binds");
        let addr = listener.local_addr().expect("the address is known");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let seen = Arc::clone(&seen);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, Arc::clone(&seen), status));
                }
            }
        });
        Self { addr, seen, task }
    }

    pub fn origin(&self) -> Url {
        Url::parse(&format!("http://{}", self.addr)).expect("a valid origin")
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("the log is readable").clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, seen: Arc<Mutex<Vec<Seen>>>, status: u16) {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    let head_end = loop {
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_owned();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => data.extend_from_slice(&buffer[..read]),
        }
    }
    seen.lock().expect("the log is writable").push(Seen {
        method,
        target,
        headers,
        body: data[head_end..head_end + length].to_vec(),
    });
    let reply =
        format!("HTTP/1.1 {status} Mock\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok");
    let _ = stream.write_all(reply.as_bytes()).await;
}

pub struct Rig {
    pub db: TestDb,
    pub store: Store,
    pub endpoint: EndpointId,
    pub keyring: Arc<Keyring>,
    pub server: MockServer,
}

pub async fn rig() -> Rig {
    rig_replying(200).await
}

pub async fn rig_replying(status: u16) -> Rig {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let record = store
        .create_endpoint(&settings(), RETENTION)
        .await
        .expect("the endpoint is created");
    Rig {
        db,
        store,
        endpoint: record.id,
        keyring: Arc::new(Keyring::parse(KEYS).expect("the key set is valid")),
        server: MockServer::start(status).await,
    }
}

fn settings() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
        verifier: None,
        templating: Default::default(),
    }
}

pub struct TestLimits {
    pub limits: NotifyLimits,
    pub tiers: Tiers,
}

pub fn limits(endpoint: i32, owner: i32, network: i32) -> TestLimits {
    limits_with_global(endpoint, owner, network, 1000)
}

pub fn limits_with_global(endpoint: i32, owner: i32, network: i32, global: i32) -> TestLimits {
    let endpoint = u32::try_from(endpoint).expect("a positive endpoint limit");
    let owner = u32::try_from(owner).expect("a positive owner limit");
    let anonymous = TierLimits {
        notify_per_endpoint_hour: Some(endpoint),
        ..TierLimits::anonymous()
    };
    let member = TierLimits {
        notify_per_endpoint_hour: Some(endpoint),
        notify_per_user_hour: Some(owner),
        ..TierLimits::member()
    };
    let tiers = Tiers::default()
        .with_limits("anonymous", anonymous)
        .and_then(|tiers| tiers.with_limits("member", member))
        .expect("the tiers are valid");
    TestLimits {
        limits: NotifyLimits {
            global_per_hour: global,
            network_per_hour: network,
        },
        tiers,
    }
}

pub struct ChannelSpec {
    pub kind: ChannelKind,
    pub config: Value,
    pub secret: Option<String>,
    pub per_hour: i32,
    pub template: Option<&'static str>,
}

impl ChannelSpec {
    pub fn url(kind: ChannelKind, url: &str) -> Self {
        Self {
            kind,
            config: serde_json::json!({ "url": "masked" }),
            secret: Some(url.to_owned()),
            per_hour: 100,
            template: None,
        }
    }
}

impl Rig {
    pub async fn channel(&self, spec: ChannelSpec) -> Uuid {
        self.channel_on(&self.endpoint, spec).await
    }

    pub async fn channel_on(&self, endpoint: &EndpointId, spec: ChannelSpec) -> Uuid {
        let id = Uuid::now_v7();
        let sealed = spec.secret.map(|url| {
            let name = channel_secret_name(id).expect("a valid name");
            let secret = Secret::new(url).expect("a valid secret");
            self.keyring
                .seal(endpoint, &name, &secret)
                .expect("the secret is sealed")
        });
        let outcome = self
            .store
            .create_channel(
                endpoint,
                RETENTION,
                &NewChannel {
                    id,
                    kind: spec.kind,
                    name: "channel".to_owned(),
                    enabled: true,
                    trigger: Trigger::Capture,
                    config: spec.config,
                    sealed,
                    template: spec.template.map(str::to_owned),
                    per_hour: spec.per_hour,
                    created_by: None,
                },
            )
            .await
            .expect("the channel is stored");
        assert!(matches!(outcome, CreateChannelOutcome::Created(_)));
        id
    }

    pub async fn token_secret(&self, name: &str, value: &str) {
        let name = SecretName::parse(name).expect("a valid name");
        let secret = Secret::new(value.to_owned()).expect("a valid secret");
        let sealed = self
            .keyring
            .seal(&self.endpoint, &name, &secret)
            .expect("the secret is sealed");
        self.store
            .put_secret(&self.endpoint, RETENTION, &name, &sealed, 20)
            .await
            .expect("the secret is stored");
    }

    pub async fn capture(&self, method: &str, path: &str, body: &'static [u8]) -> CaptureId {
        self.capture_on(&self.endpoint, method, path, body).await
    }

    pub async fn capture_with_header(&self, method: &str, path: &str, name: &str, value: &str) {
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id: CaptureId::generate().expect("an id is generated"),
                    endpoint_id: self.endpoint.clone(),
                    received_at: OffsetDateTime::now_utc(),
                    method: method.to_owned(),
                    url: format!("http://h{path}"),
                    path: path.to_owned(),
                    query_string: String::new(),
                    host: "h".to_owned(),
                    client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                    headers: vec![NameValue {
                        name: name.to_owned(),
                        value: value.to_owned(),
                    }],
                    query: Vec::new(),
                    form: None,
                    content_type: None,
                    body: Bytes::new(),
                    status_override: None,
                },
                CapturePolicy {
                    max_captures: 500,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is stored");
        assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
    }

    pub async fn capture_on(
        &self,
        endpoint: &EndpointId,
        method: &str,
        path: &str,
        body: &'static [u8],
    ) -> CaptureId {
        let id = CaptureId::generate().expect("an id is generated");
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: endpoint.clone(),
                    received_at: OffsetDateTime::now_utc(),
                    method: method.to_owned(),
                    url: format!("http://h{path}"),
                    path: path.to_owned(),
                    query_string: String::new(),
                    host: "h".to_owned(),
                    client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                    headers: vec![NameValue {
                        name: "X-Test".to_owned(),
                        value: "1".to_owned(),
                    }],
                    query: Vec::new(),
                    form: None,
                    content_type: Some("text/plain".to_owned()),
                    body: Bytes::from_static(body),
                    status_override: None,
                },
                CapturePolicy {
                    max_captures: 500,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is stored");
        assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
        id
    }

    pub async fn anonymous_endpoint(&self, network: &str) -> EndpointId {
        let net: CreatorNet = network.parse().expect("a valid network");
        self.store
            .create_endpoint_from_network(Some(&net), &settings(), RETENTION)
            .await
            .expect("the endpoint is created")
            .id
    }

    pub fn transport(&self) -> Transport {
        let mut hosts = VendorHosts::production();
        for host in VENDOR_HOSTS {
            hosts = hosts.with_override(host, self.server.origin());
        }
        Transport::new(loopback_guard(), None, hosts).expect("the transport is built")
    }

    pub fn start(&self, limits: TestLimits) -> Running {
        self.start_with(self.transport(), limits)
    }

    pub fn start_with(&self, transport: Transport, limits: TestLimits) -> Running {
        let mut settings =
            NotifySettings::new("https://hooks.example".to_owned(), limits.limits, 4, 3)
                .with_tiers(limits.tiers);
        settings.poll_interval = Duration::from_millis(100);
        let worker = NotifyWorker::new(
            self.store.clone(),
            Arc::clone(&self.keyring),
            transport,
            settings,
        )
        .expect("the worker settings are valid");
        let (stop, stopped) = oneshot::channel::<()>();
        let handle = worker.spawn(async move {
            let _ = stopped.await;
        });
        Running { stop, handle }
    }

    pub async fn count_in_state(&self, state: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE state = $1")
            .bind(state)
            .fetch_one(self.db.pool())
            .await
            .expect("the count is readable")
    }
}

pub fn loopback_guard() -> GuardConfig {
    GuardConfig {
        allow: vec!["127.0.0.0/8".parse().expect("a valid network")],
        ..GuardConfig::default()
    }
}

pub struct Running {
    stop: oneshot::Sender<()>,
    handle: NotifyHandle,
}

impl Running {
    pub async fn stop(mut self) {
        let _ = self.stop.send(());
        self.handle.join().await;
    }
}

pub async fn wait_until<F, Fut>(what: &str, mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..200 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {what}");
}
