#![allow(dead_code)]

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToSec1Point;
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
use whl_core::entropy::random_bytes;
use whl_core::response::ResponseConfig;
use whl_forward::GuardConfig;
use whl_notify::{
    NotifyHandle, NotifyLimits, NotifySettings, NotifyWorker, PushHosts, PushService, Transport,
    Vapid, VapidSubject, VendorHosts, channel_secret_name,
};
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::notify::{
    ChannelKind, CreateChannelOutcome, NewChannel, NewPushSubscription, SavePushOutcome, Trigger,
};
use whl_store::{EndpointSettings, Overflow, Store, Uuid, VerifiedIdentity};
use whl_testkit::TestDb;

pub const RETENTION: Duration = Duration::from_secs(3600);
pub const SLACK_URL: &str = "https://hooks.slack.com/services/T0/B0/slack-secret-part";
pub const DISCORD_URL: &str = "https://discord.com/api/webhooks/123456/discord-secret-part";
pub const TELEGRAM_TOKEN: &str = "123456:telegram-secret-part";
const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
pub const PUSH_HOSTS: [&str; 3] = [
    "fcm.googleapis.com",
    "updates.push.services.mozilla.com",
    "web.push.apple.com",
];
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

pub struct StatusMap {
    pub default: u16,
    pub by_target: Vec<StatusRoute>,
}

pub struct StatusRoute {
    pub target: String,
    pub status: u16,
}

impl StatusMap {
    fn status_of(&self, target: &str) -> u16 {
        self.by_target
            .iter()
            .find(|route| route.target == target)
            .map_or(self.default, |route| route.status)
    }
}

impl MockServer {
    pub async fn start(status: u16) -> Self {
        Self::start_routed(StatusMap {
            default: status,
            by_target: Vec::new(),
        })
        .await
    }

    pub async fn start_routed(statuses: StatusMap) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the mock binds");
        let addr = listener.local_addr().expect("the address is known");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let statuses = Arc::new(statuses);
        let task = tokio::spawn({
            let seen = Arc::clone(&seen);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(stream, Arc::clone(&seen), Arc::clone(&statuses)));
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

async fn serve(mut stream: TcpStream, seen: Arc<Mutex<Vec<Seen>>>, statuses: Arc<StatusMap>) {
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
    let status = statuses.status_of(&target);
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
    rig_with(MockServer::start(status).await).await
}

pub async fn rig_routed(statuses: StatusMap) -> Rig {
    rig_with(MockServer::start_routed(statuses).await).await
}

async fn rig_with(server: MockServer) -> Rig {
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
        server,
    }
}

fn settings() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
    }
}

pub fn limits(endpoint: i32, owner: i32, network: i32) -> NotifyLimits {
    limits_with_global(endpoint, owner, network, 1000)
}

pub fn limits_with_global(endpoint: i32, owner: i32, network: i32, global: i32) -> NotifyLimits {
    NotifyLimits {
        global_per_hour: global,
        endpoint_per_hour: endpoint,
        owner_per_hour: owner,
        network_per_hour: network,
    }
}

pub struct ChannelSpec {
    pub kind: ChannelKind,
    pub config: Value,
    pub secret: Option<String>,
    pub per_hour: i32,
    pub template: Option<&'static str>,
    pub created_by: Option<Uuid>,
}

impl ChannelSpec {
    pub fn url(kind: ChannelKind, url: &str) -> Self {
        Self {
            kind,
            config: serde_json::json!({ "url": "masked" }),
            secret: Some(url.to_owned()),
            per_hour: 100,
            template: None,
            created_by: None,
        }
    }

    pub fn push(created_by: Uuid) -> Self {
        Self {
            kind: ChannelKind::Push,
            config: serde_json::json!({}),
            secret: None,
            per_hour: 100,
            template: None,
            created_by: Some(created_by),
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
                    created_by: spec.created_by,
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

    pub fn start(&self, limits: NotifyLimits) -> Running {
        self.start_with(self.transport(), limits)
    }

    pub fn start_with(&self, transport: Transport, limits: NotifyLimits) -> Running {
        let mut settings = NotifySettings::new("https://hooks.example".to_owned(), limits, 4, 3);
        settings.poll_interval = Duration::from_millis(100);
        let worker = NotifyWorker::new(
            self.store.clone(),
            Arc::clone(&self.keyring),
            transport,
            settings,
        )
        .expect("the worker settings are valid");
        launch(worker)
    }

    pub fn start_push(&self, limits: NotifyLimits, service: PushService) -> Running {
        self.start_push_with(self.push_transport(), limits, service)
    }

    pub fn start_push_with(
        &self,
        transport: Transport,
        limits: NotifyLimits,
        service: PushService,
    ) -> Running {
        let mut settings = NotifySettings::new("https://hooks.example".to_owned(), limits, 4, 3);
        settings.poll_interval = Duration::from_millis(100);
        let worker = NotifyWorker::new(
            self.store.clone(),
            Arc::clone(&self.keyring),
            transport,
            settings,
        )
        .expect("the worker settings are valid")
        .with_push(service);
        launch(worker)
    }

    pub async fn owned_endpoint(&self, owner: Uuid) -> EndpointId {
        self.store
            .create_endpoint_owned(Some(owner), &settings(), RETENTION)
            .await
            .expect("the endpoint is created")
            .id
    }

    pub fn push_transport(&self) -> Transport {
        let mut hosts = VendorHosts::production();
        for host in PUSH_HOSTS {
            hosts = hosts.with_override(host, self.server.origin());
        }
        Transport::new(loopback_guard(), None, hosts).expect("the transport is built")
    }

    pub fn push_service(&self) -> PushService {
        let subject = VapidSubject::parse("mailto:ops@hooks.example").expect("a valid subject");
        let vapid = Vapid::generate(subject).expect("a key pair");
        PushService::new(vapid, PushHosts::vendors())
    }

    pub async fn user(&self, tag: &str) -> Uuid {
        self.store
            .upsert_user(&VerifiedIdentity {
                issuer: "https://sso.test".to_owned(),
                subject: format!("subject-{tag}"),
                email: format!("{tag}@hooks.example"),
                email_verified: true,
                display_name: None,
            })
            .await
            .expect("the user is stored")
            .id
    }

    pub async fn device(&self, user: Uuid, url: &str) -> Device {
        let scalar = SecretKey::from_slice(&random_bytes::<32>().expect("random bytes"))
            .expect("a valid scalar");
        let point = scalar.public_key().to_sec1_point(false);
        let auth = random_bytes::<16>().expect("random bytes").to_vec();
        let outcome = self
            .store
            .save_push_subscription(&NewPushSubscription {
                id: Uuid::now_v7(),
                user_id: user,
                endpoint_url: url.to_owned(),
                host: Url::parse(url)
                    .expect("a valid endpoint URL")
                    .host_str()
                    .expect("an endpoint host")
                    .to_owned(),
                p256dh: point.as_bytes().to_vec(),
                auth: auth.clone(),
                user_agent: None,
            })
            .await
            .expect("the subscription is stored");
        let SavePushOutcome::Saved(saved) = outcome else {
            panic!("the subscription was not saved: {outcome:?}");
        };
        Device {
            scalar: scalar.to_bytes().to_vec(),
            auth,
            subscription: saved.id,
        }
    }

    pub async fn subscription_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM push_subscriptions")
            .fetch_one(self.db.pool())
            .await
            .expect("the count is readable")
    }

    pub async fn delivery_outcome(&self) -> DeliveryOutcome {
        let row: (String, Option<i16>, Option<String>, i16) = sqlx::query_as(
            "SELECT state, status, error, attempts FROM notification_deliveries \
             ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(self.db.pool())
        .await
        .expect("a delivery exists");
        DeliveryOutcome {
            state: row.0,
            status: row.1,
            error: row.2,
            attempts: row.3,
        }
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

pub struct Device {
    pub scalar: Vec<u8>,
    pub auth: Vec<u8>,
    pub subscription: Uuid,
}

#[derive(Debug)]
pub struct DeliveryOutcome {
    pub state: String,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub attempts: i16,
}

fn launch(worker: NotifyWorker) -> Running {
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = worker.spawn(async move {
        let _ = stopped.await;
    });
    Running { stop, handle }
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
