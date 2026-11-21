use std::future::Future;
use std::time::Duration;

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;
use whl_telemetry::series;

use crate::captures::EVENT_CHANNEL;
use crate::store::Store;

pub const LEADER_LOCK_KEY: i64 = 0x7768_6c5f_6d61_696e;
pub const HARD_LIMIT_PERCENT: u128 = 110;
pub const CAP_PERCENT: u128 = 100;
pub const DEFAULT_DELETED_GRACE: Duration = Duration::from_secs(10);
pub const DEFAULT_SESSION_IDLE: Duration = Duration::from_secs(12 * 3600);
pub const DEFAULT_PURGE_BATCH: u32 = 50;
pub const DEFAULT_EVICT_BATCH: u32 = 5;
pub const DEFAULT_STATEMENT_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_CYCLE_INTERVAL: Duration = Duration::from_secs(30);
pub const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(10);
pub const DEFAULT_LOCK_CHECK_INTERVAL: Duration = Duration::from_secs(5);
pub const MAX_PURGE_BATCHES_PER_CYCLE: u32 = 200;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const LEADER_APPLICATION_NAME: &str = "whl-maintenance-leader";

const TRY_LOCK_SQL: &str = "SELECT pg_try_advisory_lock($1)";
const UNLOCK_SQL: &str = "SELECT pg_advisory_unlock($1)";
const HOLDS_LOCK_SQL: &str = "SELECT EXISTS ( \
    SELECT 1 FROM pg_locks \
    WHERE locktype = 'advisory' AND granted AND pid = pg_backend_pid() \
      AND ((classid::bigint << 32) | objid::bigint) = $1)";

const PURGE_SQL: &str = "WITH victims AS ( \
        SELECT id, (deleted_at IS NOT NULL) AS was_deleted FROM endpoints \
        WHERE (deleted_at IS NOT NULL AND deleted_at <= now() - make_interval(secs => $2)) \
           OR last_active_at <= now() - make_interval(secs => $1) \
        LIMIT $3 \
        FOR UPDATE SKIP LOCKED \
    ), purged AS ( \
        DELETE FROM endpoints e USING victims v WHERE e.id = v.id \
        RETURNING e.id, v.was_deleted \
    ) \
    SELECT id, was_deleted FROM purged";

const EVICT_SQL: &str = "WITH ranked AS ( \
        SELECT id, \
               sum(bytes_used) OVER (ORDER BY last_active_at ASC, id ASC) - bytes_used AS freed_before \
        FROM endpoints WHERE bytes_used > 0 \
    ), picked AS ( \
        SELECT id FROM ranked WHERE freed_before < $2 ORDER BY freed_before ASC LIMIT $1 \
    ), victims AS ( \
        SELECT id, bytes_used FROM endpoints \
        WHERE id IN (SELECT id FROM picked) \
        FOR UPDATE SKIP LOCKED \
    ), evicted AS ( \
        DELETE FROM endpoints e USING victims v WHERE e.id = v.id \
        RETURNING e.id, v.bytes_used \
    ) \
    SELECT id, bytes_used FROM evicted";

const NOTIFY_MANY_SQL: &str = "SELECT pg_notify($1, payload) FROM unnest($2::text[]) AS payload";

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceError {
    #[error("maintenance query failed: {0}")]
    Query(#[from] sqlx::Error),
    #[error("cannot encode a notification: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("the leader connection did not answer within {0:?}")]
    LeaderTimeout(Duration),
    #[error("a stored figure is outside the supported range")]
    OutOfRange,
    #[error("the maintenance task has stopped")]
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceSettings {
    pub retention: Duration,
    pub cap_bytes: u64,
    pub deleted_grace: Duration,
    pub session_idle: Duration,
    pub purge_batch: u32,
    pub evict_batch: u32,
    pub statement_timeout: Duration,
    pub cycle_interval: Duration,
    pub retry_interval: Duration,
    pub lock_check_interval: Duration,
}

impl MaintenanceSettings {
    pub fn new(retention: Duration, cap_bytes: u64) -> Self {
        Self {
            retention,
            cap_bytes,
            deleted_grace: DEFAULT_DELETED_GRACE,
            session_idle: DEFAULT_SESSION_IDLE,
            purge_batch: DEFAULT_PURGE_BATCH,
            evict_batch: DEFAULT_EVICT_BATCH,
            statement_timeout: DEFAULT_STATEMENT_TIMEOUT,
            cycle_interval: DEFAULT_CYCLE_INTERVAL,
            retry_interval: DEFAULT_RETRY_INTERVAL,
            lock_check_interval: DEFAULT_LOCK_CHECK_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PurgeBatch {
    pub expired: u64,
    pub deleted: u64,
}

impl PurgeBatch {
    pub fn total(&self) -> u64 {
        self.expired + self.deleted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvictBatch {
    pub endpoints: u64,
    pub bytes: u64,
}

pub fn exceeds_hard_limit(used: u64, cap: u64) -> bool {
    u128::from(used) * 100 > u128::from(cap) * HARD_LIMIT_PERCENT
}

pub fn exceeds_cap(used: u64, cap: u64) -> bool {
    u128::from(used) * 100 > u128::from(cap) * CAP_PERCENT
}

pub fn below_cap(used: u64, cap: u64) -> bool {
    u128::from(used) * 100 < u128::from(cap) * CAP_PERCENT
}

impl Store {
    pub async fn storage_bytes(&self) -> Result<u64, MaintenanceError> {
        let total: i64 =
            sqlx::query_scalar("SELECT COALESCE(sum(bytes_used), 0)::bigint FROM endpoints")
                .fetch_one(self.pool())
                .await?;
        u64::try_from(total).map_err(|_| MaintenanceError::OutOfRange)
    }

    pub async fn live_endpoint_count(&self) -> Result<u64, MaintenanceError> {
        let total: i64 =
            sqlx::query_scalar("SELECT count(*) FROM endpoints WHERE deleted_at IS NULL")
                .fetch_one(self.pool())
                .await?;
        u64::try_from(total).map_err(|_| MaintenanceError::OutOfRange)
    }

    pub async fn purge_batch(
        &self,
        settings: &MaintenanceSettings,
    ) -> Result<PurgeBatch, MaintenanceError> {
        let mut transaction = self.pool().begin().await?;
        apply_statement_timeout(&mut transaction, settings.statement_timeout).await?;
        let rows = sqlx::query(PURGE_SQL)
            .bind(settings.retention.as_secs_f64())
            .bind(settings.deleted_grace.as_secs_f64())
            .bind(i64::from(settings.purge_batch))
            .fetch_all(&mut *transaction)
            .await?;
        let mut batch = PurgeBatch::default();
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row.try_get("id")?;
            let was_deleted: bool = row.try_get("was_deleted")?;
            if was_deleted {
                batch.deleted += 1;
            } else {
                batch.expired += 1;
            }
            ids.push(id);
        }
        notify_deleted(&mut transaction, &ids).await?;
        transaction.commit().await?;
        Ok(batch)
    }

    pub async fn evict_batch(
        &self,
        settings: &MaintenanceSettings,
        shortfall_bytes: u64,
    ) -> Result<EvictBatch, MaintenanceError> {
        let mut transaction = self.pool().begin().await?;
        apply_statement_timeout(&mut transaction, settings.statement_timeout).await?;
        let rows = sqlx::query(EVICT_SQL)
            .bind(i64::from(settings.evict_batch))
            .bind(i64::try_from(shortfall_bytes).unwrap_or(i64::MAX))
            .fetch_all(&mut *transaction)
            .await?;
        let mut batch = EvictBatch::default();
        let mut ids = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row.try_get("id")?;
            let bytes: i64 = row.try_get("bytes_used")?;
            batch.endpoints += 1;
            batch.bytes = batch
                .bytes
                .saturating_add(u64::try_from(bytes).map_err(|_| MaintenanceError::OutOfRange)?);
            ids.push(id);
        }
        notify_deleted(&mut transaction, &ids).await?;
        transaction.commit().await?;
        Ok(batch)
    }
}

async fn apply_statement_timeout(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    timeout: Duration,
) -> Result<(), MaintenanceError> {
    sqlx::query("SELECT set_config('statement_timeout', $1, true)")
        .bind(timeout.as_millis().to_string())
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn notify_deleted(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: &[String],
) -> Result<(), MaintenanceError> {
    if ids.is_empty() {
        return Ok(());
    }
    let payloads = ids
        .iter()
        .map(|id| serde_json::to_string(&serde_json::json!({ "t": "endpoint_deleted", "e": id })))
        .collect::<Result<Vec<_>, _>>()?;
    sqlx::query(NOTIFY_MANY_SQL)
        .bind(EVENT_CHANNEL)
        .bind(payloads)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum LeadershipLost {
    #[error("the leader connection failed: {0}")]
    Connection(#[from] sqlx::Error),
    #[error("the leader connection did not answer within {0:?}")]
    Timeout(Duration),
    #[error("the leader lock is no longer held")]
    NotHeld,
}

struct LeaderLock {
    connection: PgConnection,
}

impl LeaderLock {
    async fn acquire(store: &Store) -> Result<Option<Self>, MaintenanceError> {
        let options = store
            .pool()
            .connect_options()
            .as_ref()
            .clone()
            .application_name(LEADER_APPLICATION_NAME);
        let mut connection =
            tokio::time::timeout(CONNECT_TIMEOUT, PgConnection::connect_with(&options))
                .await
                .map_err(|_| MaintenanceError::LeaderTimeout(CONNECT_TIMEOUT))??;
        let acquired = tokio::time::timeout(
            LOCK_QUERY_TIMEOUT,
            sqlx::query_scalar::<_, bool>(TRY_LOCK_SQL)
                .bind(LEADER_LOCK_KEY)
                .fetch_one(&mut connection),
        )
        .await
        .map_err(|_| MaintenanceError::LeaderTimeout(LOCK_QUERY_TIMEOUT))??;
        if acquired {
            Ok(Some(Self { connection }))
        } else {
            if let Err(error) = connection.close().await {
                tracing::debug!(%error, "closing the connection of a lost election failed");
            }
            Ok(None)
        }
    }

    async fn verify(&mut self) -> Result<(), LeadershipLost> {
        let held = tokio::time::timeout(
            LOCK_QUERY_TIMEOUT,
            sqlx::query_scalar::<_, bool>(HOLDS_LOCK_SQL)
                .bind(LEADER_LOCK_KEY)
                .fetch_one(&mut self.connection),
        )
        .await
        .map_err(|_| LeadershipLost::Timeout(LOCK_QUERY_TIMEOUT))??;
        if held {
            Ok(())
        } else {
            Err(LeadershipLost::NotHeld)
        }
    }

    async fn release(mut self) {
        let unlocked = tokio::time::timeout(
            LOCK_QUERY_TIMEOUT,
            sqlx::query_scalar::<_, bool>(UNLOCK_SQL)
                .bind(LEADER_LOCK_KEY)
                .fetch_one(&mut self.connection),
        )
        .await;
        match unlocked {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => tracing::warn!("the leader lock was not held at release"),
            Ok(Err(error)) => tracing::warn!(%error, "releasing the leader lock failed"),
            Err(_) => tracing::warn!("releasing the leader lock timed out"),
        }
        match tokio::time::timeout(LOCK_QUERY_TIMEOUT, self.connection.close()).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "closing the leader connection failed"),
            Err(_) => tracing::warn!("closing the leader connection timed out"),
        }
    }
}

#[derive(Debug)]
pub struct MaintenanceHandle {
    task: JoinHandle<()>,
    leader: watch::Receiver<bool>,
}

impl MaintenanceHandle {
    pub fn is_leader(&self) -> bool {
        *self.leader.borrow()
    }

    pub async fn wait_for_leadership(&self, leading: bool) -> Result<(), MaintenanceError> {
        let mut leader = self.leader.clone();
        leader
            .wait_for(|current| *current == leading)
            .await
            .map(|_| ())
            .map_err(|_| MaintenanceError::Stopped)
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn join(self) {
        if let Err(error) = self.task.await {
            tracing::error!(%error, "the maintenance task ended abnormally");
        }
    }
}

pub fn spawn_maintenance(
    store: Store,
    settings: MaintenanceSettings,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> MaintenanceHandle {
    let (leader_tx, leader) = watch::channel(false);
    let task = tokio::spawn(run(store, settings, shutdown, leader_tx));
    MaintenanceHandle { task, leader }
}

fn publish_leadership(leader: &watch::Sender<bool>, leading: bool) {
    leader.send_replace(leading);
    metrics::gauge!(series::MAINTENANCE_LEADER).set(if leading { 1.0 } else { 0.0 });
}

async fn run(
    store: Store,
    settings: MaintenanceSettings,
    shutdown: impl Future<Output = ()>,
    leader: watch::Sender<bool>,
) {
    tokio::pin!(shutdown);
    publish_leadership(&leader, false);
    loop {
        let election = tokio::select! {
            () = &mut shutdown => return,
            election = LeaderLock::acquire(&store) => election,
        };
        match election {
            Ok(Some(mut lock)) => {
                publish_leadership(&leader, true);
                tracing::info!("this replica is the maintenance leader");
                let lost = tokio::select! {
                    () = &mut shutdown => None,
                    lost = lead(&store, &settings, &mut lock) => Some(lost),
                };
                publish_leadership(&leader, false);
                match lost {
                    None => {
                        lock.release().await;
                        tracing::info!("the maintenance leader released its lock");
                        return;
                    }
                    Some(error) => {
                        tracing::warn!(%error, "maintenance leadership lost");
                    }
                }
            }
            Ok(None) => tracing::debug!("another replica is the maintenance leader"),
            Err(error) => tracing::warn!(%error, "the maintenance election failed"),
        }
        tokio::select! {
            () = &mut shutdown => return,
            () = tokio::time::sleep(settings.retry_interval) => {}
        }
    }
}

async fn lead(
    store: &Store,
    settings: &MaintenanceSettings,
    lock: &mut LeaderLock,
) -> LeadershipLost {
    let mut next_cycle = Instant::now();
    loop {
        if let Err(lost) = lock.verify().await {
            return lost;
        }
        if Instant::now() >= next_cycle {
            if let Err(lost) = cycle(store, settings, lock).await {
                return lost;
            }
            next_cycle = Instant::now() + settings.cycle_interval;
        }
        let until_cycle = next_cycle.saturating_duration_since(Instant::now());
        tokio::time::sleep(until_cycle.min(settings.lock_check_interval)).await;
    }
}

async fn cycle(
    store: &Store,
    settings: &MaintenanceSettings,
    lock: &mut LeaderLock,
) -> Result<(), LeadershipLost> {
    purge(store, settings, lock).await?;
    evict(store, settings, lock).await?;
    expire_logins(store, settings).await;
    expire_usage(store).await;
    match store.live_endpoint_count().await {
        Ok(count) => metrics::gauge!(series::ENDPOINTS).set(count as f64),
        Err(error) => tracing::warn!(%error, "counting endpoints failed"),
    }
    Ok(())
}

async fn expire_logins(store: &Store, settings: &MaintenanceSettings) {
    match store.delete_expired_sessions(settings.session_idle).await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "deleted expired sessions"),
        Err(error) => tracing::warn!(%error, "deleting expired sessions failed"),
    }
    match store.delete_expired_login_states().await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "deleted expired login states"),
        Err(error) => tracing::warn!(%error, "deleting expired login states failed"),
    }
}

async fn expire_usage(store: &Store) {
    match store.delete_old_usage_windows().await {
        Ok(0) => {}
        Ok(deleted) => tracing::info!(deleted, "deleted old usage windows"),
        Err(error) => tracing::warn!(%error, "deleting old usage windows failed"),
    }
}

async fn purge(
    store: &Store,
    settings: &MaintenanceSettings,
    lock: &mut LeaderLock,
) -> Result<(), LeadershipLost> {
    for _ in 0..MAX_PURGE_BATCHES_PER_CYCLE {
        lock.verify().await?;
        let batch = match store.purge_batch(settings).await {
            Ok(batch) => batch,
            Err(error) => {
                tracing::warn!(%error, "a purge batch failed");
                return Ok(());
            }
        };
        if batch.total() > 0 {
            tracing::info!(
                expired = batch.expired,
                deleted = batch.deleted,
                "purged endpoints"
            );
            metrics::counter!(series::EVICTIONS_TOTAL, "reason" => "expired")
                .increment(batch.expired);
            metrics::counter!(series::EVICTIONS_TOTAL, "reason" => "deleted")
                .increment(batch.deleted);
        }
        if batch.total() < u64::from(settings.purge_batch) {
            return Ok(());
        }
    }
    Ok(())
}

async fn evict(
    store: &Store,
    settings: &MaintenanceSettings,
    lock: &mut LeaderLock,
) -> Result<(), LeadershipLost> {
    let mut used = match store.storage_bytes().await {
        Ok(used) => used,
        Err(error) => {
            tracing::warn!(%error, "reading the storage figure failed");
            return Ok(());
        }
    };
    if !exceeds_cap(used, settings.cap_bytes) {
        return Ok(());
    }
    while !below_cap(used, settings.cap_bytes) {
        lock.verify().await?;
        let shortfall = used - settings.cap_bytes + 1;
        let batch = match store.evict_batch(settings, shortfall).await {
            Ok(batch) => batch,
            Err(error) => {
                tracing::warn!(%error, "an eviction batch failed");
                return Ok(());
            }
        };
        if batch.endpoints == 0 {
            tracing::warn!(
                used,
                cap = settings.cap_bytes,
                "storage is over the cap and no endpoint can be evicted"
            );
            return Ok(());
        }
        used = used.saturating_sub(batch.bytes);
        tracing::info!(
            endpoints = batch.endpoints,
            freed_bytes = batch.bytes,
            used_bytes = used,
            cap_bytes = settings.cap_bytes,
            "evicted the least recently active endpoints"
        );
        metrics::counter!(series::EVICTIONS_TOTAL, "reason" => "cap").increment(batch.endpoints);
    }
    Ok(())
}
