use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use metrics::{counter, gauge, histogram};
use thiserror::Error;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use whl_store::Store;
use whl_store::forward::{AttemptRecord, ClaimPolicy, ClaimedJob, RecordOutcome, Settlement};
use whl_telemetry::series::{
    FORWARD_ATTEMPTS_TOTAL, FORWARD_DURATION_SECONDS, FORWARD_IN_FLIGHT, FORWARD_QUEUE_DEPTH,
};

use crate::client::GuardedClient;
use crate::outcome::{self, Finished, Verdict};
use crate::request::build_request;
use crate::retry::RetryPolicy;

pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const MAX_CLAIM_BATCH: usize = 16;
pub const MIN_LEASE_MARGIN: Duration = Duration::from_secs(5);
pub const MAX_ATTEMPTS_LIMIT: u32 = 100;

const DEADLINE_GRACE: Duration = Duration::from_secs(1);
const QUEUE_DEPTH_INTERVAL: Duration = Duration::from_secs(10);
const RECORD_TRIES: u32 = 3;
const RECORD_RETRY_DELAY: Duration = Duration::from_millis(500);
const WAKE_RECONNECT_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorkerError {
    #[error("the concurrency limit must be at least 1")]
    Concurrency,
    #[error("the attempt limit must be between 1 and {MAX_ATTEMPTS_LIMIT}")]
    MaxAttempts,
    #[error("the poll interval must not be zero")]
    PollInterval,
    #[error(
        "the lease of {lease:?} must exceed the attempt timeout of {timeout:?} by at least {MIN_LEASE_MARGIN:?}"
    )]
    Lease { lease: Duration, timeout: Duration },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerConfig {
    pub concurrency: usize,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub retry: RetryPolicy,
}

impl WorkerConfig {
    pub fn new(concurrency: usize, max_attempts: u32) -> Self {
        Self {
            concurrency,
            lease: DEFAULT_LEASE,
            poll_interval: DEFAULT_POLL_INTERVAL,
            retry: RetryPolicy::new(max_attempts),
        }
    }

    fn validate(&self, attempt_timeout: Duration) -> Result<(), WorkerError> {
        if self.concurrency == 0 {
            return Err(WorkerError::Concurrency);
        }
        if self.retry.max_attempts == 0 || self.retry.max_attempts > MAX_ATTEMPTS_LIMIT {
            return Err(WorkerError::MaxAttempts);
        }
        if self.poll_interval.is_zero() {
            return Err(WorkerError::PollInterval);
        }
        if self.lease < attempt_timeout.saturating_add(MIN_LEASE_MARGIN) {
            return Err(WorkerError::Lease {
                lease: self.lease,
                timeout: attempt_timeout,
            });
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct WorkerHandle {
    task: JoinHandle<()>,
}

impl WorkerHandle {
    pub async fn join(&mut self) {
        if let Err(error) = (&mut self.task).await {
            tracing::error!(%error, "the forward worker ended abnormally");
        }
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

#[derive(Debug)]
pub struct ForwardWorker {
    store: Store,
    client: GuardedClient,
    config: WorkerConfig,
}

struct Shared {
    store: Store,
    client: GuardedClient,
    config: WorkerConfig,
    wake: Notify,
    slots: Arc<Semaphore>,
}

impl ForwardWorker {
    pub fn new(
        store: Store,
        client: GuardedClient,
        config: WorkerConfig,
    ) -> Result<Self, WorkerError> {
        config.validate(attempt_deadline(&client))?;
        Ok(Self {
            store,
            client,
            config,
        })
    }

    pub fn spawn(self, shutdown: impl Future<Output = ()> + Send + 'static) -> WorkerHandle {
        WorkerHandle {
            task: tokio::spawn(self.run(shutdown)),
        }
    }

    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let shared = Arc::new(Shared {
            slots: Arc::new(Semaphore::new(self.config.concurrency)),
            store: self.store,
            client: self.client,
            config: self.config,
            wake: Notify::new(),
        });
        let listener = tokio::spawn(listen_for_wakeups(Arc::clone(&shared)));
        let mut attempts = JoinSet::new();
        tracing::info!(
            concurrency = shared.config.concurrency,
            max_attempts = shared.config.retry.max_attempts,
            "the forward worker started"
        );
        tokio::pin!(shutdown);
        claim_loop(&shared, &mut attempts, &mut shutdown).await;
        listener.abort();
        tracing::info!(
            in_flight = attempts.len(),
            "the forward worker stopped claiming"
        );
        while let Some(joined) = attempts.join_next().await {
            report_join(joined);
        }
        tracing::info!("the forward worker finished");
    }
}

async fn claim_loop(
    shared: &Arc<Shared>,
    attempts: &mut JoinSet<()>,
    shutdown: &mut (impl Future<Output = ()> + Unpin),
) {
    let mut last_depth: Option<Instant> = None;
    let mut last_reap: Option<Instant> = None;
    loop {
        while let Some(joined) = attempts.try_join_next() {
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
        if due(&mut last_reap, shared.config.poll_interval) {
            reap(shared).await;
        }
        if due(&mut last_depth, QUEUE_DEPTH_INTERVAL) {
            publish_queue_depth(shared).await;
        }
        let requested = permits.len();
        let claimed = claim(shared, requested).await;
        let drained = claimed.as_ref().is_ok_and(|jobs| jobs.len() == requested);
        match claimed {
            Ok(jobs) => {
                for (job, permit) in jobs.into_iter().zip(permits.drain(..)) {
                    attempts.spawn(run_attempt(Arc::clone(shared), job, permit));
                }
            }
            Err(error) => tracing::warn!(%error, "cannot claim forward jobs"),
        }
        drop(permits);
        if drained {
            continue;
        }
        let stopped = tokio::select! {
            biased;
            () = &mut *shutdown => true,
            () = shared.wake.notified() => false,
            () = tokio::time::sleep(shared.config.poll_interval) => false,
        };
        if stopped {
            return;
        }
    }
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
) -> Result<Vec<ClaimedJob>, whl_store::forward::ForwardStoreError> {
    shared
        .store
        .claim_forward_jobs(ClaimPolicy {
            limit: i64::try_from(requested).unwrap_or(i64::MAX),
            max_attempts: i32::try_from(shared.config.retry.max_attempts).unwrap_or(i32::MAX),
            lease: shared.config.lease,
        })
        .await
}

async fn reap(shared: &Shared) {
    let max_attempts = i32::try_from(shared.config.retry.max_attempts).unwrap_or(i32::MAX);
    match shared.store.fail_exhausted_forward_jobs(max_attempts).await {
        Ok(0) => {}
        Ok(count) => tracing::warn!(count, "failed forward jobs whose last lease expired"),
        Err(error) => tracing::warn!(%error, "cannot fail exhausted forward jobs"),
    }
}

async fn publish_queue_depth(shared: &Shared) {
    match shared.store.forward_queue_depth().await {
        Ok(depth) => gauge!(FORWARD_QUEUE_DEPTH).set(depth as f64),
        Err(error) => tracing::warn!(%error, "cannot read the forward queue depth"),
    }
}

async fn listen_for_wakeups(shared: Arc<Shared>) {
    loop {
        match shared.store.listen_for_forward_wakeups().await {
            Ok(mut listener) => {
                shared.wake.notify_one();
                loop {
                    match listener.wait().await {
                        Ok(()) => shared.wake.notify_one(),
                        Err(error) => {
                            tracing::warn!(%error, "the forward wake listener lost its connection");
                            break;
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "cannot listen for forward wakeups"),
        }
        tokio::time::sleep(WAKE_RECONNECT_DELAY).await;
    }
}

fn report_join(joined: Result<(), tokio::task::JoinError>) {
    if let Err(error) = joined {
        tracing::error!(%error, "a forward attempt task ended abnormally");
    }
}

struct InFlight;

impl InFlight {
    fn enter() -> Self {
        gauge!(FORWARD_IN_FLIGHT).increment(1.0);
        Self
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        gauge!(FORWARD_IN_FLIGHT).decrement(1.0);
    }
}

fn attempt_deadline(client: &GuardedClient) -> Duration {
    client.config().total_timeout.saturating_add(DEADLINE_GRACE)
}

async fn run_attempt(shared: Arc<Shared>, job: ClaimedJob, permit: OwnedSemaphorePermit) {
    let _slot = permit;
    let _in_flight = InFlight::enter();
    let capture = match shared.store.load_job_capture(job.capture_id).await {
        Ok(Some(capture)) => capture,
        Ok(None) => {
            tracing::debug!(job_id = %job.id, "the capture of a claimed job is gone");
            return;
        }
        Err(error) => {
            tracing::warn!(job_id = %job.id, %error, "cannot load the capture of a claimed job");
            return;
        }
    };
    let started = Instant::now();
    let finished = match build_request(&job, &capture, shared.client.config()) {
        Err(error) => outcome::from_request_error(job.id, job.attempt, started.elapsed(), &error),
        Ok(request) => {
            let sent = tokio::time::timeout(
                attempt_deadline(&shared.client),
                shared.client.send(request),
            )
            .await
            .unwrap_or(Err(crate::error::ForwardError::TotalTimeout));
            match sent {
                Ok(response) => {
                    outcome::from_response(job.id, job.attempt, started.elapsed(), &response)
                }
                Err(error) => outcome::from_error(job.id, job.attempt, started.elapsed(), &error),
            }
        }
    };
    let settlement = settle(&shared.config.retry, job.attempt, &finished);
    counter!(FORWARD_ATTEMPTS_TOTAL, "outcome" => finished.outcome.label()).increment(1);
    histogram!(FORWARD_DURATION_SECONDS).record(started.elapsed().as_secs_f64());
    store_result(&shared, &finished.record, settlement).await;
}

fn settle(policy: &RetryPolicy, attempt: i16, finished: &Finished) -> Settlement {
    match finished.verdict {
        Verdict::Success => Settlement::Succeeded,
        Verdict::Final => Settlement::Failed,
        Verdict::Retryable => {
            let attempt = u32::try_from(attempt).unwrap_or(u32::MAX);
            if policy.has_attempts_left(attempt) {
                Settlement::RetryIn(policy.delay_after(attempt))
            } else {
                Settlement::Failed
            }
        }
    }
}

async fn store_result(shared: &Shared, record: &AttemptRecord, settlement: Settlement) {
    for tries in 1..=RECORD_TRIES {
        match shared
            .store
            .record_forward_attempt(record, settlement)
            .await
        {
            Ok(RecordOutcome::Recorded) => return,
            Ok(RecordOutcome::Superseded) => {
                tracing::warn!(
                    job_id = %record.job_id,
                    attempt = record.attempt,
                    "the job was reclaimed or deleted before its result was recorded"
                );
                return;
            }
            Err(error) => {
                tracing::warn!(job_id = %record.job_id, tries, %error, "cannot record a forward attempt");
                tokio::time::sleep(RECORD_RETRY_DELAY * tries).await;
            }
        }
    }
    tracing::error!(
        job_id = %record.job_id,
        attempt = record.attempt,
        "the forward attempt result was lost; the job is retried after its lease expires"
    );
}
