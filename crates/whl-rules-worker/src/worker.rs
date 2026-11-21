use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use metrics::gauge;
use thiserror::Error;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use whl_forward::GuardedClient;
use whl_script::{EngineLimits, LimitsError, ScriptCompiler, ScriptPool};
use whl_secrets::Keyring;
use whl_store::Store;
use whl_store::rule_exec::{ClaimedRun, RunClaimPolicy};
use whl_telemetry::series::RULE_QUEUE_DEPTH;

use crate::run;
use crate::settings::{MAX_CLAIM_BATCH, SettingsError, WorkerSettings};

const QUEUE_DEPTH_INTERVAL: Duration = Duration::from_secs(10);
const REAP_INTERVAL: Duration = Duration::from_secs(5);
const WAKE_RECONNECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("the worker settings are invalid: {0}")]
    Settings(#[from] SettingsError),
    #[error("the script engine limits are invalid: {0}")]
    Scripts(#[from] LimitsError),
}

#[derive(Debug)]
pub struct RulesHandle {
    task: JoinHandle<()>,
}

impl RulesHandle {
    pub async fn join(&mut self) {
        if let Err(error) = (&mut self.task).await {
            tracing::error!(%error, "the rules worker ended abnormally");
        }
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

pub(crate) struct Shared {
    pub store: Store,
    pub client: GuardedClient,
    pub keyring: Option<Arc<Keyring>>,
    pub pool: Option<Arc<ScriptPool>>,
    pub compiler: ScriptCompiler,
    pub settings: WorkerSettings,
    pub wake: Notify,
    pub slots: Arc<Semaphore>,
    pub stopping: watch::Receiver<bool>,
}

pub struct RulesWorker {
    store: Store,
    client: GuardedClient,
    keyring: Option<Arc<Keyring>>,
    pool: Option<Arc<ScriptPool>>,
    compiler: ScriptCompiler,
    settings: WorkerSettings,
}

impl RulesWorker {
    pub fn new(
        store: Store,
        client: GuardedClient,
        keyring: Option<Arc<Keyring>>,
        pool: Option<Arc<ScriptPool>>,
        settings: WorkerSettings,
    ) -> Result<Self, WorkerError> {
        settings.validate()?;
        Ok(Self {
            store,
            client,
            keyring,
            pool,
            compiler: ScriptCompiler::new(EngineLimits::default())?,
            settings,
        })
    }

    pub fn spawn(self, shutdown: impl Future<Output = ()> + Send + 'static) -> RulesHandle {
        RulesHandle {
            task: tokio::spawn(self.run(shutdown)),
        }
    }

    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let (stop, stopping) = watch::channel(false);
        let shared = Arc::new(Shared {
            slots: Arc::new(Semaphore::new(self.settings.concurrency)),
            store: self.store,
            client: self.client,
            keyring: self.keyring,
            pool: self.pool,
            compiler: self.compiler,
            settings: self.settings,
            wake: Notify::new(),
            stopping,
        });
        let listener = tokio::spawn(listen_for_wakeups(Arc::clone(&shared)));
        let mut runs = JoinSet::new();
        tracing::info!(
            concurrency = shared.settings.concurrency,
            max_attempts = shared.settings.retry.max_attempts,
            "the rules worker started"
        );
        tokio::pin!(shutdown);
        claim_loop(&shared, &mut runs, &mut shutdown).await;
        listener.abort();
        let _ = stop.send(true);
        tracing::info!(in_flight = runs.len(), "the rules worker stopped claiming");
        while let Some(joined) = runs.join_next().await {
            report_join(joined);
        }
        tracing::info!("the rules worker finished");
    }
}

async fn claim_loop(
    shared: &Arc<Shared>,
    runs: &mut JoinSet<()>,
    shutdown: &mut (impl Future<Output = ()> + Unpin),
) {
    let mut last_depth: Option<Instant> = None;
    let mut last_reap: Option<Instant> = None;
    loop {
        while let Some(joined) = runs.try_join_next() {
            report_join(joined);
        }
        let first = tokio::select! {
            biased;
            () = &mut *shutdown => return,
            permit = Arc::clone(&shared.slots).acquire_owned() => permit,
        };
        let Ok(first) = first else {
            return;
        };
        let mut permits = vec![first];
        while permits.len() < MAX_CLAIM_BATCH {
            match Arc::clone(&shared.slots).try_acquire_owned() {
                Ok(permit) => permits.push(permit),
                Err(_) => break,
            }
        }
        if due(&mut last_reap, REAP_INTERVAL) {
            reap(shared).await;
        }
        if due(&mut last_depth, QUEUE_DEPTH_INTERVAL) {
            publish_depth(shared).await;
        }
        let requested = permits.len();
        let claimed = claim(shared, requested).await;
        let drained = claimed.as_ref().is_ok_and(|batch| batch.len() == requested);
        match claimed {
            Ok(batch) => {
                for (claimed_run, permit) in batch.into_iter().zip(permits.drain(..)) {
                    runs.spawn(execute(Arc::clone(shared), claimed_run, permit));
                }
            }
            Err(error) => tracing::warn!(%error, "cannot claim rule runs"),
        }
        drop(permits);
        if drained {
            continue;
        }
        let stopped = tokio::select! {
            biased;
            () = &mut *shutdown => true,
            () = shared.wake.notified() => false,
            () = tokio::time::sleep(shared.settings.poll_interval) => false,
        };
        if stopped {
            return;
        }
    }
}

async fn execute(shared: Arc<Shared>, claimed: ClaimedRun, permit: OwnedSemaphorePermit) {
    let _slot = permit;
    run::execute(&shared, &claimed).await;
}

fn due(last: &mut Option<Instant>, every: Duration) -> bool {
    let now = Instant::now();
    match last {
        Some(previous) if now.duration_since(*previous) < every => false,
        _ => {
            *last = Some(now);
            true
        }
    }
}

async fn claim(
    shared: &Shared,
    requested: usize,
) -> Result<Vec<ClaimedRun>, whl_store::rules::RuleStoreError> {
    shared
        .store
        .claim_rule_runs(RunClaimPolicy {
            limit: i64::try_from(requested).unwrap_or(i64::MAX),
            per_endpoint: shared.settings.runs_per_endpoint,
            max_claims: shared.settings.max_claims,
            lease: shared.settings.lease,
        })
        .await
}

async fn reap(shared: &Shared) {
    match shared
        .store
        .fail_exhausted_rule_runs(shared.settings.max_claims)
        .await
    {
        Ok(0) => {}
        Ok(count) => tracing::warn!(count, "failed rule runs whose last lease expired"),
        Err(error) => tracing::warn!(%error, "cannot fail exhausted rule runs"),
    }
}

async fn publish_depth(shared: &Shared) {
    match shared.store.rule_queue_depth().await {
        Ok(depth) => gauge!(RULE_QUEUE_DEPTH).set(depth as f64),
        Err(error) => tracing::warn!(%error, "cannot read the rule queue depth"),
    }
}

async fn listen_for_wakeups(shared: Arc<Shared>) {
    loop {
        match shared.store.listen_for_rule_wakeups().await {
            Ok(mut listener) => {
                shared.wake.notify_one();
                loop {
                    match listener.wait().await {
                        Ok(()) => shared.wake.notify_one(),
                        Err(error) => {
                            tracing::warn!(%error, "the rules wake listener lost its connection");
                            break;
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "cannot listen for rule wakeups"),
        }
        tokio::time::sleep(WAKE_RECONNECT_DELAY).await;
    }
}

fn report_join(joined: Result<(), tokio::task::JoinError>) {
    if let Err(error) = joined {
        tracing::error!(%error, "a rule run task ended abnormally");
    }
}
