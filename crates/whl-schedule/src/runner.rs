use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use metrics::counter;
use time::OffsetDateTime;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};
use whl_capture::StorageFull;
use whl_forward::GuardedClient;
use whl_secrets::Keyring;
use whl_store::schedules::{
    ClaimedSchedule, NewScheduleRun, RunOutcome, ScheduleClaimPolicy, ScheduleSettlement,
    ScheduleStoreError, SettleOutcome,
};
use whl_store::{Store, Uuid};
use whl_telemetry::series::SCHEDULE_RUNS_TOTAL;

use crate::clock::Clock;
use crate::cron::CronSchedule;
use crate::outcome::{pause_reason, progress, run_error};
use crate::run::{Executed, execute};

pub const DEFAULT_LEASE: Duration = Duration::from_secs(60);
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const MAX_CLAIM_BATCH: usize = 8;
pub const MIN_LEASE_MARGIN: Duration = Duration::from_secs(10);

const DEADLINE_GRACE: Duration = Duration::from_secs(5);
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(20);
const SETTLE_TRIES: u32 = 3;
const SETTLE_RETRY_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RunnerError {
    #[error("the batch size must be between 1 and {MAX_CLAIM_BATCH}")]
    Batch,
    #[error("the poll interval must not be zero")]
    PollInterval,
    #[error("the run limit per hour must be at least 1")]
    RunsPerHour,
    #[error(
        "the lease of {lease:?} must exceed the run deadline of {deadline:?} by at least {MIN_LEASE_MARGIN:?}"
    )]
    Lease { lease: Duration, deadline: Duration },
}

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub batch: usize,
    pub lease: Duration,
    pub poll_interval: Duration,
    pub min_interval: Duration,
    pub runs_per_hour: u32,
    pub retention: Duration,
    pub max_captures: u32,
    pub public_url: String,
}

impl RunnerConfig {
    pub fn new(
        retention: Duration,
        max_captures: u32,
        public_url: &str,
        min_interval: Duration,
        runs_per_hour: u32,
    ) -> Self {
        Self {
            batch: MAX_CLAIM_BATCH,
            lease: DEFAULT_LEASE,
            poll_interval: DEFAULT_POLL_INTERVAL,
            min_interval,
            runs_per_hour,
            retention,
            max_captures,
            public_url: public_url.trim_end_matches('/').to_owned(),
        }
    }

    fn validate(&self, deadline: Duration) -> Result<(), RunnerError> {
        if self.batch == 0 || self.batch > MAX_CLAIM_BATCH {
            return Err(RunnerError::Batch);
        }
        if self.poll_interval.is_zero() {
            return Err(RunnerError::PollInterval);
        }
        if self.runs_per_hour == 0 {
            return Err(RunnerError::RunsPerHour);
        }
        if self.lease < deadline.saturating_add(MIN_LEASE_MARGIN) {
            return Err(RunnerError::Lease {
                lease: self.lease,
                deadline,
            });
        }
        Ok(())
    }
}

pub(crate) struct Inner {
    pub store: Store,
    pub client: GuardedClient,
    pub keyring: Option<Arc<Keyring>>,
    pub clock: Arc<dyn Clock>,
    pub storage_full: StorageFull,
    pub config: RunnerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub schedule_id: Uuid,
    pub outcome: RunOutcome,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub capture_id: Option<Uuid>,
    pub paused: bool,
    pub settled: SettleOutcome,
}

#[derive(Debug)]
pub struct RunnerHandle {
    task: JoinHandle<()>,
}

impl RunnerHandle {
    pub async fn join(&mut self) {
        if let Err(error) = (&mut self.task).await {
            tracing::error!(%error, "the schedule runner ended abnormally");
        }
    }

    pub fn abort(&self) {
        self.task.abort();
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

pub struct ScheduleRunner {
    inner: Arc<Inner>,
}

impl ScheduleRunner {
    pub fn new(
        store: Store,
        client: GuardedClient,
        keyring: Option<Arc<Keyring>>,
        clock: Arc<dyn Clock>,
        storage_full: StorageFull,
        config: RunnerConfig,
    ) -> Result<Self, RunnerError> {
        config.validate(run_deadline(&client))?;
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                client,
                keyring,
                clock,
                storage_full,
                config,
            }),
        })
    }

    pub fn spawn(self, shutdown: impl Future<Output = ()> + Send + 'static) -> RunnerHandle {
        RunnerHandle {
            task: tokio::spawn(self.run(shutdown)),
        }
    }

    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        let batch = self.inner.config.batch;
        tracing::info!(batch, "the schedule runner started");
        let slots = Arc::new(Semaphore::new(batch));
        let mut runs: JoinSet<Option<RunReport>> = JoinSet::new();
        tokio::pin!(shutdown);
        loop {
            while let Some(joined) = runs.try_join_next() {
                report_join(joined);
            }
            let free = slots.available_permits();
            let mut filled = false;
            if free > 0 {
                match self.claim_into(&mut runs, Some(&slots), free).await {
                    Ok(claimed) => filled = claimed == free,
                    Err(error) => tracing::warn!(%error, "cannot claim due schedules"),
                }
            }
            if filled {
                tokio::select! {
                    biased;
                    () = &mut shutdown => break,
                    () = std::future::ready(()) => continue,
                }
            }
            tokio::select! {
                biased;
                () = &mut shutdown => break,
                Some(joined) = runs.join_next(), if !runs.is_empty() => report_join(joined),
                () = tokio::time::sleep(self.inner.config.poll_interval) => {}
            }
        }
        tracing::info!(
            in_flight = runs.len(),
            "the schedule runner stopped claiming"
        );
        let drain = async {
            while let Some(joined) = runs.join_next().await {
                report_join(joined);
            }
        };
        if tokio::time::timeout(SHUTDOWN_DRAIN, drain).await.is_err() {
            tracing::warn!(
                "schedule runs were still in flight at shutdown, their leases will expire"
            );
            runs.abort_all();
        }
        tracing::info!("the schedule runner finished");
    }

    pub async fn tick(&self) -> Result<Vec<RunReport>, ScheduleStoreError> {
        let mut runs = JoinSet::new();
        self.claim_into(&mut runs, None, self.inner.config.batch)
            .await?;
        let mut reports = Vec::new();
        while let Some(joined) = runs.join_next().await {
            match joined {
                Ok(Some(report)) => reports.push(report),
                Ok(None) => {}
                Err(error) => tracing::error!(%error, "a schedule run ended abnormally"),
            }
        }
        Ok(reports)
    }

    async fn claim_into(
        &self,
        runs: &mut JoinSet<Option<RunReport>>,
        slots: Option<&Arc<Semaphore>>,
        limit: usize,
    ) -> Result<usize, ScheduleStoreError> {
        let claimed = self
            .inner
            .store
            .claim_due_schedules(ScheduleClaimPolicy {
                now: self.inner.clock.now(),
                limit: i64::try_from(limit).unwrap_or(i64::MAX),
                lease: self.inner.config.lease,
                retention: self.inner.config.retention,
            })
            .await?;
        let count = claimed.len();
        for schedule in claimed {
            let permit = match slots {
                Some(slots) => Arc::clone(slots).acquire_owned().await.ok(),
                None => None,
            };
            runs.spawn(run_claimed(Arc::clone(&self.inner), schedule, permit));
        }
        Ok(count)
    }
}

fn report_join(joined: Result<Option<RunReport>, tokio::task::JoinError>) {
    if let Err(error) = joined {
        tracing::error!(%error, "a schedule run ended abnormally");
    }
}

fn run_deadline(client: &GuardedClient) -> Duration {
    client.config().total_timeout.saturating_add(DEADLINE_GRACE)
}

async fn run_claimed(
    inner: Arc<Inner>,
    claimed: ClaimedSchedule,
    _permit: Option<OwnedSemaphorePermit>,
) -> Option<RunReport> {
    let started = inner.clock.now();
    let deadline = inner
        .config
        .lease
        .saturating_sub(MIN_LEASE_MARGIN)
        .max(run_deadline(&inner.client));
    let executed = match tokio::time::timeout(deadline, execute(&inner, &claimed, started)).await {
        Ok(Ok(executed)) => executed,
        Ok(Err(aborted)) => {
            tracing::warn!(schedule = %claimed.id, %aborted, "a schedule run was abandoned, its lease will expire");
            return None;
        }
        Err(_) => {
            tracing::warn!(schedule = %claimed.id, "a schedule run exceeded its deadline, its lease will expire");
            return None;
        }
    };
    settle(&inner, &claimed, started, executed).await
}

async fn settle(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    started: OffsetDateTime,
    executed: Executed,
) -> Option<RunReport> {
    let advance = progress(claimed.consecutive_failures, executed.outcome);
    let finished = inner.clock.now();
    let mut paused_reason = advance
        .paused
        .then(|| pause_reason(advance.failures, executed.error.as_deref()))
        .or_else(|| executed.pause.clone());
    let mut next_run_at = None;
    if paused_reason.is_none() {
        match next_occurrence(inner, claimed, finished) {
            Ok(next) => next_run_at = Some(next),
            Err(reason) => paused_reason = Some(run_error(&reason)),
        }
    }
    let paused = paused_reason.is_some();
    let last_run_at = (executed.outcome != RunOutcome::Skipped).then_some(started);
    let settlement = ScheduleSettlement {
        id: claimed.id,
        lease: claimed.lease,
        scheduled_for: claimed.scheduled_for,
        last_run_at,
        consecutive_failures: advance.failures,
        paused_reason,
        next_run_at,
        run: NewScheduleRun {
            id: Uuid::now_v7(),
            started_at: started,
            duration_ms: executed.duration_ms,
            outcome: executed.outcome,
            status: executed
                .status
                .and_then(|status| i16::try_from(status).ok()),
            error: executed.error.clone(),
            capture_id: executed.capture_id,
        },
    };
    let settled = settle_with_retries(inner, claimed, &settlement).await?;
    counter!(SCHEDULE_RUNS_TOTAL, "outcome" => executed.outcome.as_str()).increment(1);
    if paused && settled == SettleOutcome::Settled {
        counter!(SCHEDULE_RUNS_TOTAL, "outcome" => "paused").increment(1);
        tracing::warn!(schedule = %claimed.id, "a schedule paused itself after repeated failures");
    }
    Some(RunReport {
        schedule_id: claimed.id,
        outcome: executed.outcome,
        status: executed.status,
        error: executed.error,
        capture_id: executed.capture_id,
        paused,
        settled,
    })
}

fn next_occurrence(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    finished: OffsetDateTime,
) -> Result<OffsetDateTime, String> {
    let schedule = CronSchedule::parse(&claimed.cron, &claimed.timezone)
        .map_err(|error| format!("the stored schedule cannot be evaluated: {error}"))?;
    let next = schedule
        .next_after(finished)
        .map_err(|error| format!("the stored schedule cannot be evaluated: {error}"))?;
    let floor = claimed.scheduled_for + inner.config.min_interval;
    Ok(next.max(floor))
}

async fn settle_with_retries(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    settlement: &ScheduleSettlement,
) -> Option<SettleOutcome> {
    for attempt in 1..=SETTLE_TRIES {
        match inner.store.settle_schedule(settlement).await {
            Ok(SettleOutcome::Settled) => return Some(SettleOutcome::Settled),
            Ok(SettleOutcome::LeaseLost) => {
                tracing::warn!(schedule = %claimed.id, "the lease of a schedule run was lost before it was settled");
                return Some(SettleOutcome::LeaseLost);
            }
            Err(error) => {
                tracing::warn!(schedule = %claimed.id, attempt, %error, "cannot settle a schedule run");
                tokio::time::sleep(SETTLE_RETRY_DELAY * attempt).await;
            }
        }
    }
    tracing::error!(schedule = %claimed.id, "a schedule run could not be settled, it is repeated after its lease expires");
    None
}
