#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use whl_capture::StorageFull;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_forward::{GuardConfig, GuardedClient};
use whl_schedule::{Clock, RunnerConfig, ScheduleRunner};
use whl_secrets::Keyring;
use whl_store::schedules::{CreateScheduleOutcome, NewSchedule, ScheduleRecord, ScheduleRequest};
use whl_store::{EndpointSettings, Overflow, Store, Uuid, VerifiedIdentity};
use whl_testkit::TestDb;

pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);
pub const KEYRING: &str = "1:a2tra2tra2tra2tra2tra2tra2tra2tra2tra2tra2s=";

pub struct TestClock(Mutex<OffsetDateTime>);

impl TestClock {
    pub fn starting_now() -> Arc<Self> {
        let now = OffsetDateTime::now_utc();
        let minute = now.unix_timestamp() - now.unix_timestamp().rem_euclid(60);
        Arc::new(Self(Mutex::new(
            OffsetDateTime::from_unix_timestamp(minute).expect("a valid time"),
        )))
    }

    pub fn now_value(&self) -> OffsetDateTime {
        *self.0.lock().expect("the clock is not poisoned")
    }

    pub fn set(&self, moment: OffsetDateTime) {
        *self.0.lock().expect("the clock is not poisoned") = moment;
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.0.lock().expect("the clock is not poisoned");
        *now += by;
    }
}

impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().expect("the clock is not poisoned")
    }
}

pub struct Scene {
    pub db: TestDb,
    pub store: Store,
    pub owner: Uuid,
    pub endpoint: EndpointId,
    pub clock: Arc<TestClock>,
}

pub async fn scene() -> Scene {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = store
        .upsert_user(&VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: "owner".to_owned(),
            email: "owner@example.test".to_owned(),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored")
        .id;
    let endpoint = store
        .create_endpoint_owned(
            Some(owner),
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                cors: false,
                overflow: Overflow::Refuse,
                tags: Vec::new(),
                verifier: None,
                templating: Default::default(),
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is stored")
        .id;
    Scene {
        db,
        store,
        owner,
        endpoint,
        clock: TestClock::starting_now(),
    }
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

pub fn client(guard: GuardConfig) -> GuardedClient {
    GuardedClient::new(guard).expect("the client builds")
}

pub fn config(max_captures: u32, runs_per_hour: u32) -> RunnerConfig {
    RunnerConfig::new(
        RETENTION,
        max_captures,
        "https://whl.test",
        Duration::from_secs(1),
        runs_per_hour,
    )
}

pub fn runner(scene: &Scene, guard: GuardConfig, keyring: Option<Arc<Keyring>>) -> ScheduleRunner {
    runner_with(scene, guard, keyring, config(500, 1000))
}

pub fn runner_with(
    scene: &Scene,
    guard: GuardConfig,
    keyring: Option<Arc<Keyring>>,
    config: RunnerConfig,
) -> ScheduleRunner {
    ScheduleRunner::new(
        scene.store.clone(),
        client(guard),
        keyring,
        scene.clock.clone(),
        StorageFull::default(),
        config,
    )
    .expect("the runner settings are valid")
}

pub fn runner_full(scene: &Scene, storage_full: StorageFull) -> ScheduleRunner {
    ScheduleRunner::new(
        scene.store.clone(),
        client(loopback_guard()),
        None,
        scene.clock.clone(),
        storage_full,
        config(500, 1000),
    )
    .expect("the runner settings are valid")
}

pub fn get(url: &str) -> ScheduleRequest {
    ScheduleRequest {
        method: "GET".to_owned(),
        url: url.to_owned(),
        headers: Vec::new(),
        body: None,
    }
}

pub async fn add_schedule(
    scene: &Scene,
    cron: &str,
    request: ScheduleRequest,
    next_run_at: Option<OffsetDateTime>,
) -> ScheduleRecord {
    let new = NewSchedule {
        created_by: scene.owner,
        name: "probe".to_owned(),
        cron: cron.to_owned(),
        timezone: "UTC".to_owned(),
        request,
        enabled: true,
        next_run_at,
    };
    match scene
        .store
        .create_schedule(&scene.endpoint, RETENTION, &new, 100)
        .await
        .expect("the schedule is stored")
    {
        CreateScheduleOutcome::Created(record) => *record,
        other => panic!("the schedule was not created: {other:?}"),
    }
}

pub async fn reload(scene: &Scene, schedule: &ScheduleRecord) -> ScheduleRecord {
    scene
        .store
        .get_schedule(&scene.endpoint, schedule.id)
        .await
        .expect("the schedule is read")
        .expect("the schedule exists")
}

#[derive(Debug)]
pub struct StoredCapture {
    pub kind: String,
    pub status: i16,
    pub method: String,
    pub url: String,
    pub host: String,
    pub meta: Value,
    pub body: Vec<u8>,
    pub headers: String,
}

pub async fn captures(scene: &Scene) -> Vec<StoredCapture> {
    let rows = sqlx::query_as::<_, (String, i16, String, String, String, String, Vec<u8>, String)>(
        "SELECT c.kind, c.response_status, c.method, c.url, c.host, c.meta::text, b.data, \
         c.headers::text \
         FROM captures c JOIN capture_bodies b ON b.capture_id = c.id \
         WHERE c.endpoint_id = $1 ORDER BY c.seq",
    )
    .bind(scene.endpoint.unpack())
    .fetch_all(scene.db.pool())
    .await
    .expect("the captures are read");
    rows.into_iter()
        .map(
            |(kind, status, method, url, host, meta, body, headers)| StoredCapture {
                kind,
                status,
                method,
                url,
                host,
                meta: serde_json::from_str(&meta).expect("the meta is JSON"),
                body,
                headers,
            },
        )
        .collect()
}

pub struct Seen {
    pub head: String,
}

pub struct Target {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Target {
    pub async fn start(handler: impl Fn(&Seen) -> (u16, String) + Send + Sync + 'static) -> Self {
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
                    tokio::spawn(async move { serve(stream, seen, handler).await });
                }
            }
        });
        Self { addr, seen, task }
    }

    pub async fn tarpit() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the target binds");
        let addr = listener.local_addr().expect("the target has an address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                held.push(stream);
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

    pub fn heads(&self) -> Vec<String> {
        self.seen.lock().expect("the log is not poisoned").clone()
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    mut stream: TcpStream,
    seen: Arc<Mutex<Vec<String>>>,
    handler: Arc<impl Fn(&Seen) -> (u16, String)>,
) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
    let head = String::from_utf8_lossy(&buffer).into_owned();
    seen.lock()
        .expect("the log is not poisoned")
        .push(head.clone());
    let (status, body) = handler(&Seen { head });
    let reply = format!(
        "HTTP/1.1 {status} Test\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
}
