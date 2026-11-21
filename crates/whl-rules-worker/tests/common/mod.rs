#![allow(dead_code)]

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde_json::Value;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_core::tier::{ANONYMOUS_TIER, TierLimits, Tiers};
use whl_forward::{GuardConfig, GuardedClient, RetryPolicy};
use whl_rules::{References, validate_set};
use whl_rules_worker::{RulesHandle, RulesWorker, WorkerSettings};
use whl_script::{EngineLimits, ScriptCompiler};
use whl_secrets::{Keyring, Secret, SecretName};
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::rules::{RuleChange, WriteOutcome};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

pub const RETENTION: Duration = Duration::from_secs(3600);
pub const KEYRING: &str = "1:a2tra2tra2tra2tra2tra2tra2tra2tra2tra2tra2s=";
const WAIT: Duration = Duration::from_secs(20);

pub struct Scene {
    pub db: TestDb,
    pub store: Store,
    pub endpoint: EndpointId,
}

pub async fn scene() -> Scene {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let endpoint = new_endpoint(&store).await;
    Scene {
        db,
        store,
        endpoint,
    }
}

pub async fn new_endpoint(store: &Store) -> EndpointId {
    store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                templating: Default::default(),
                cors: false,
                overflow: Overflow::Refuse,
                tags: Vec::new(),
                verifier: None,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created")
        .id
}

pub fn keyring() -> Arc<Keyring> {
    Arc::new(Keyring::parse(KEYRING).expect("a valid keyring"))
}

pub fn loopback_guard() -> GuardConfig {
    GuardConfig {
        allow: vec!["127.0.0.1/32".parse().expect("a valid network")],
        connect_timeout: Duration::from_secs(2),
        hop_timeout: Duration::from_secs(5),
        total_timeout: Duration::from_secs(10),
        max_redirects: 0,
        ..GuardConfig::default()
    }
}

pub fn tiers_with(name: &str, change: impl FnOnce(&mut TierLimits)) -> Tiers {
    let mut limits = match name {
        ANONYMOUS_TIER => TierLimits::anonymous(),
        _ => TierLimits::member(),
    };
    change(&mut limits);
    Tiers::default()
        .with_limits(name, limits)
        .expect("the tier limits are valid")
}

pub fn settings() -> WorkerSettings {
    let mut settings = WorkerSettings::new("https://whl.test", RETENTION, 4, 3, Tiers::default());
    settings.retry = RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(20),
        multiplier: 2,
        max_backoff: Duration::from_millis(60),
        jitter_percent: 0,
    };
    settings.poll_interval = Duration::from_millis(50);
    settings
}

pub fn fake_value(parts: &[&str]) -> String {
    parts.concat()
}

pub struct Running {
    stop: Option<oneshot::Sender<()>>,
    handle: RulesHandle,
}

impl Running {
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.handle.join().await;
    }
}

pub fn start(
    scene: &Scene,
    guard: GuardConfig,
    settings: WorkerSettings,
    keyring: Option<Arc<Keyring>>,
) -> Running {
    start_observed(scene, guard, settings, keyring, Arc::new(Notify::new()))
}

pub fn start_observed(
    scene: &Scene,
    guard: GuardConfig,
    settings: WorkerSettings,
    keyring: Option<Arc<Keyring>>,
    observed: Arc<Notify>,
) -> Running {
    let worker = RulesWorker::new(
        scene.store.clone(),
        GuardedClient::new(guard).expect("the client builds"),
        keyring,
        None,
        settings,
    )
    .expect("the worker settings are valid");
    let (stop, stopped) = oneshot::channel::<()>();
    let handle = worker.spawn(async move {
        let _ = stopped.await;
        observed.notify_one();
    });
    Running {
        stop: Some(stop),
        handle,
    }
}

pub fn header(name: &str, value: &str) -> NameValue {
    NameValue {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

impl Scene {
    pub async fn add_rule(&self, input: Value, secrets: &[&str]) {
        let scripts =
            ScriptCompiler::new(EngineLimits::default()).expect("the default limits are valid");
        let channels: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM notification_channels")
            .fetch_all(self.db.pool())
            .await
            .expect("the channels are readable");
        let references = References {
            secrets: secrets.iter().map(|name| (*name).to_owned()).collect(),
            forward_rules: BTreeSet::new(),
            channels: channels.into_iter().collect(),
        };
        let rule = validate_set(&[input], &references, &scripts)
            .expect("the rule is valid")
            .remove(0);
        let version: i32 = sqlx::query_scalar("SELECT rules_version FROM endpoints WHERE id = $1")
            .bind(self.endpoint.unpack())
            .fetch_one(self.db.pool())
            .await
            .expect("the version is readable");
        let outcome = self
            .store
            .write_rules(
                &self.endpoint,
                RETENTION,
                version,
                &RuleChange::Create(rule),
            )
            .await
            .expect("the rule is written");
        assert!(matches!(outcome, WriteOutcome::Written(_)), "{outcome:?}");
    }

    pub async fn put_secret(&self, name: &str, value: &str) {
        let name = SecretName::parse(name).expect("a valid secret name");
        let secret = Secret::new(value.to_owned()).expect("a valid secret value");
        let sealed = keyring()
            .seal(&self.endpoint, &name, &secret)
            .expect("the secret seals");
        self.store
            .put_secret(&self.endpoint, RETENTION, &name, &sealed, 20)
            .await
            .expect("the secret is stored");
    }

    pub async fn capture(&self, headers: Vec<NameValue>) -> CaptureId {
        let id = CaptureId::generate().expect("entropy is available");
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: self.endpoint.clone(),
                    received_at: OffsetDateTime::now_utc(),
                    method: "POST".to_owned(),
                    url: "http://h/x".to_owned(),
                    path: "/x".to_owned(),
                    query_string: String::new(),
                    host: "h".to_owned(),
                    client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                    headers,
                    query: Vec::new(),
                    form: None,
                    content_type: Some("text/plain".to_owned()),
                    body: Bytes::from_static(b"hello"),
                    status_override: None,
                },
                CapturePolicy {
                    max_captures: 500,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is stored");
        assert!(
            matches!(outcome, CaptureOutcome::Stored { .. }),
            "the capture was not stored: {outcome:?}"
        );
        id
    }

    pub async fn run(&self, capture: &CaptureId) -> RunRow {
        self.run_where(capture, "trigger = 'capture'").await
    }

    async fn run_where(&self, capture: &CaptureId, filter: &str) -> RunRow {
        let sql = format!(
            "SELECT id, state, error, step_attempts, next_step, attempts, \
               log::text AS log, vars::text AS vars, step_results::text AS steps \
             FROM rule_runs WHERE capture_id = $1 AND {filter}"
        );
        let row: (
            Uuid,
            String,
            Option<String>,
            i16,
            i16,
            i16,
            String,
            String,
            String,
        ) = sqlx::query_as(&sql)
            .bind(capture.unpack())
            .fetch_one(self.db.pool())
            .await
            .expect("the run row is readable");
        RunRow {
            id: row.0,
            state: row.1,
            error: row.2,
            step_attempts: row.3,
            next_step: row.4,
            attempts: row.5,
            log: serde_json::from_str(&row.6).expect("the log is JSON"),
            vars: serde_json::from_str(&row.7).expect("the vars are JSON"),
            steps: row.8,
        }
    }

    pub async fn wait_for_final(&self, capture: &CaptureId) -> RunRow {
        self.wait_for(capture, "trigger = 'capture'", &["succeeded", "failed"])
            .await
    }

    pub async fn wait_for_rerun(&self, capture: &CaptureId) -> RunRow {
        self.wait_for(capture, "trigger = 'rerun'", &["succeeded", "failed"])
            .await
    }

    pub async fn wait_for(&self, capture: &CaptureId, filter: &str, states: &[&str]) -> RunRow {
        let started = Instant::now();
        loop {
            let row = self.run_where(capture, filter).await;
            if states.contains(&row.state.as_str()) {
                return row;
            }
            assert!(
                started.elapsed() < WAIT,
                "the run stayed {} ({:?}): {}",
                row.state,
                row.error,
                row.log
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(self.db.pool())
            .await
            .expect("the count is readable")
    }

    pub async fn execute(&self, sql: &str) {
        sqlx::query(sql)
            .execute(self.db.pool())
            .await
            .expect("the statement runs");
    }
}

#[derive(Debug)]
pub struct RunRow {
    pub id: Uuid,
    pub state: String,
    pub error: Option<String>,
    pub step_attempts: i16,
    pub next_step: i16,
    pub attempts: i16,
    pub log: Value,
    pub vars: Value,
    pub steps: String,
}

impl RunRow {
    pub fn step_entries(&self) -> Vec<&Value> {
        self.log
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|entry| entry.get("step").is_some())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn everything(&self) -> String {
        format!("{}{}{}{:?}", self.log, self.vars, self.steps, self.error)
    }
}

pub struct Seen {
    pub head: String,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn request_line(&self) -> &str {
        self.head.lines().next().unwrap_or_default()
    }

    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }
}

pub struct Target {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Arc<Seen>>>>,
    task: JoinHandle<()>,
}

impl Target {
    pub async fn start(handler: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static) -> Self {
        Self::spawn(handler, None).await
    }

    pub async fn held(release: Arc<Notify>) -> Self {
        Self::spawn(|_| (200, "held".to_owned()), Some(release)).await
    }

    async fn spawn(
        handler: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static,
        hold: Option<Arc<Notify>>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the target binds");
        let addr = listener.local_addr().expect("the target has an address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let task = tokio::spawn({
            let seen = Arc::clone(&seen);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let seen = Arc::clone(&seen);
                    let handler = Arc::clone(&handler);
                    let hold = hold.clone();
                    tokio::spawn(async move { serve(stream, seen, handler, hold).await });
                }
            }
        });
        Self { addr, seen, task }
    }

    pub async fn status(code: u16) -> Self {
        Self::start(move |_| (code, "target body".to_owned())).await
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn hits(&self) -> usize {
        self.seen.lock().expect("the log is not poisoned").len()
    }

    pub fn seen(&self) -> Vec<Arc<Seen>> {
        self.seen.lock().expect("the log is not poisoned").clone()
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a port binds");
    let port = listener
        .local_addr()
        .expect("the port has an address")
        .port();
    drop(listener);
    port
}

async fn serve(
    mut stream: TcpStream,
    seen: Arc<Mutex<Vec<Arc<Seen>>>>,
    handler: Arc<impl Fn(&Seen) -> (u16, String)>,
    hold: Option<Arc<Notify>>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
    let body = buffer
        .get(head_end..(head_end + length).min(buffer.len()))
        .unwrap_or_default()
        .to_vec();
    let request = Arc::new(Seen { head, body });
    seen.lock()
        .expect("the log is not poisoned")
        .push(Arc::clone(&request));
    if let Some(release) = hold {
        release.notified().await;
    }
    let (status, text) = handler(&request);
    let response = format!(
        "HTTP/1.1 {status} Test\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}
