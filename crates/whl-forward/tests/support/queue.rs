use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use sqlx::PgPool;
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_forward::{
    ForwardWorker, GuardedClient, RetryPolicy, StaticLookup, WorkerConfig, WorkerHandle,
};
use whl_store::Uuid;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NameValue, NewCapture};
use whl_store::forward::{CreateRuleOutcome, ForwardRule, NewForwardRule};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

use super::{loopback_config, read_request};

pub const RETENTION: Duration = Duration::from_secs(3600);

pub struct Fixture {
    pub db: TestDb,
    pub store: Store,
    pub endpoint: EndpointId,
}

pub async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let record = store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                cors: false,
                overflow: Overflow::Refuse,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    Fixture {
        db,
        store,
        endpoint: record.id,
    }
}

impl Fixture {
    pub fn pool(&self) -> &PgPool {
        self.db.pool()
    }

    pub async fn rule(&self, target: &str, allowlist: &[&str]) -> ForwardRule {
        let outcome = self
            .store
            .create_forward_rule(
                &self.endpoint,
                RETENTION,
                &NewForwardRule {
                    enabled: true,
                    target_url: target.to_owned(),
                    method: None,
                    content_type: None,
                    header_allowlist: allowlist.iter().map(|name| (*name).to_owned()).collect(),
                    append_path: false,
                },
            )
            .await
            .expect("the rule is stored");
        match outcome {
            CreateRuleOutcome::Created(rule) => rule,
            other => panic!("the rule was not created: {other:?}"),
        }
    }

    pub async fn capture(&self, headers: &[(&str, &str)], body: &'static [u8]) -> CaptureId {
        let id = CaptureId::generate().expect("an id is generated");
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
                    headers: headers
                        .iter()
                        .map(|(name, value)| NameValue {
                            name: (*name).to_owned(),
                            value: (*value).to_owned(),
                        })
                        .collect(),
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

    pub async fn job_ids(&self) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT id FROM forward_jobs ORDER BY created_at, id")
            .fetch_all(self.pool())
            .await
            .expect("the jobs are readable")
    }

    pub async fn state_of(&self, job: Uuid) -> String {
        sqlx::query_scalar("SELECT state FROM forward_jobs WHERE id = $1")
            .bind(job)
            .fetch_one(self.pool())
            .await
            .expect("the job is readable")
    }

    pub async fn count_in_state(&self, state: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM forward_jobs WHERE state = $1")
            .bind(state)
            .fetch_one(self.pool())
            .await
            .expect("the jobs are countable")
    }

    pub async fn attempt_count(&self, job: Uuid) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM forward_attempts WHERE job_id = $1")
            .bind(job)
            .fetch_one(self.pool())
            .await
            .expect("the attempts are countable")
    }
}

pub fn worker_config(concurrency: usize, max_attempts: u32) -> WorkerConfig {
    let mut config = WorkerConfig::new(concurrency, max_attempts);
    config.poll_interval = Duration::from_millis(30);
    config.retry = RetryPolicy {
        max_attempts,
        initial_backoff: Duration::from_millis(50),
        multiplier: 2,
        max_backoff: Duration::from_millis(200),
        jitter_percent: 0,
    };
    config
}

pub fn worker_for(store: &Store, config: WorkerConfig) -> ForwardWorker {
    worker_with_lookup(store, config, StaticLookup::new())
}

pub fn worker_with_lookup(
    store: &Store,
    config: WorkerConfig,
    lookup: StaticLookup,
) -> ForwardWorker {
    let client = GuardedClient::with_lookup(loopback_config(), Arc::new(lookup))
        .expect("the client is configured");
    ForwardWorker::new(store.clone(), client, config).expect("the worker is configured")
}

pub fn unrestricted_worker(
    store: &Store,
    config: WorkerConfig,
    lookup: StaticLookup,
) -> ForwardWorker {
    let mut guard = loopback_config();
    guard.allow.clear();
    let client =
        GuardedClient::with_lookup(guard, Arc::new(lookup)).expect("the client is configured");
    ForwardWorker::new(store.clone(), client, config).expect("the worker is configured")
}

pub struct Running {
    handle: WorkerHandle,
    stop: watch::Sender<bool>,
}

pub fn run(worker: ForwardWorker) -> Running {
    let (stop, mut stopped) = watch::channel(false);
    let handle = worker.spawn(async move {
        let _ = stopped.wait_for(|flag| *flag).await;
    });
    Running { handle, stop }
}

impl Running {
    pub async fn stop(mut self) {
        let _ = self.stop.send(true);
        tokio::time::timeout(Duration::from_secs(10), self.handle.join())
            .await
            .expect("the worker stops in time");
    }

    pub fn trigger_stop(&self) {
        let _ = self.stop.send(true);
    }

    pub async fn join(&mut self) {
        tokio::time::timeout(Duration::from_secs(10), self.handle.join())
            .await
            .expect("the worker stops in time");
    }
}

pub async fn wait_until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for _ in 0..500 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

pub struct HoldingServer {
    pub addr: SocketAddr,
    current: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl HoldingServer {
    pub async fn start(hold: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the holding server binds");
        let addr = listener.local_addr().expect("the address is known");
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn({
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            let total = Arc::clone(&total);
            async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let current = Arc::clone(&current);
                    let peak = Arc::clone(&peak);
                    let total = Arc::clone(&total);
                    tokio::spawn(async move {
                        if read_request(&mut stream).await.is_none() {
                            return;
                        }
                        total.fetch_add(1, Ordering::SeqCst);
                        let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(hold).await;
                        current.fetch_sub(1, Ordering::SeqCst);
                        let _ = stream
                            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                            .await;
                    });
                }
            }
        });
        Self {
            addr,
            current,
            peak,
            total,
            task,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }

    pub fn in_flight(&self) -> usize {
        self.current.load(Ordering::SeqCst)
    }
}

impl Drop for HoldingServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
