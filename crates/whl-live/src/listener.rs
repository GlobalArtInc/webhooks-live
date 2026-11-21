use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::{PgListener, PgNotification};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, MissedTickBehavior};
use whl_store::Store;
use whl_store::stream::HEARTBEAT_NOTIFICATION;

use crate::event::{DecodeError, Notification, ResyncReason};
use crate::hub::Hub;
use crate::{emit, jitter};

pub const CHANNEL: &str = "whl_events";
pub const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
pub const MAX_BACKOFF: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const STABLE_AFTER: Duration = Duration::from_secs(30);

const BACKOFF_JITTER: f64 = 0.2;
const SILENCE_FACTOR: u32 = 2;
const NOTIFY: &str = "SELECT pg_notify($1, $2)";

#[derive(Debug, thiserror::Error)]
pub enum ListenerError {
    #[error("the listener task has stopped")]
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerSettings {
    pub heartbeat_interval: Duration,
    pub stable_after: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for ListenerSettings {
    fn default() -> Self {
        Self {
            heartbeat_interval: HEARTBEAT_INTERVAL,
            stable_after: STABLE_AFTER,
            initial_backoff: INITIAL_BACKOFF,
            max_backoff: MAX_BACKOFF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    current: Duration,
    max: Duration,
    initial: Duration,
}

impl Backoff {
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            current: initial,
            max,
            initial,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = self.current.saturating_mul(2).min(self.max);
        delay
    }

    pub fn reset(&mut self) {
        self.current = self.initial;
    }

    pub fn settle(&mut self, connected_for: Duration, stable_after: Duration) {
        if connected_for >= stable_after {
            self.reset();
        }
    }
}

#[derive(Debug, Clone)]
pub struct Liveness {
    exited: watch::Receiver<bool>,
}

impl Liveness {
    pub fn is_alive(&self) -> bool {
        !*self.exited.borrow()
    }

    pub async fn stopped(&self) {
        let mut exited = self.exited.clone();
        let _ = exited.wait_for(|exited| *exited).await;
    }
}

#[derive(Debug)]
pub struct ListenerHandle {
    task: JoinHandle<()>,
    established: watch::Receiver<u64>,
    liveness: Liveness,
}

impl ListenerHandle {
    pub fn establishments(&self) -> u64 {
        *self.established.borrow()
    }

    pub async fn wait_for_establishments(&self, count: u64) -> Result<(), ListenerError> {
        let mut established = self.established.clone();
        established
            .wait_for(|current| *current >= count)
            .await
            .map(|_| ())
            .map_err(|_| ListenerError::Stopped)
    }

    pub async fn wait_listening(&self) -> Result<(), ListenerError> {
        self.wait_for_establishments(1).await
    }

    pub fn liveness(&self) -> Liveness {
        self.liveness.clone()
    }

    pub async fn join(self) {
        if let Err(error) = self.task.await {
            tracing::error!(%error, "the listener task ended abnormally");
        }
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

#[derive(Debug)]
struct ExitSignal(watch::Sender<bool>);

impl Drop for ExitSignal {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

pub fn spawn_listener(
    store: Store,
    hub: Arc<Hub>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> ListenerHandle {
    spawn_listener_with(store, hub, ListenerSettings::default(), shutdown)
}

pub fn spawn_listener_with(
    store: Store,
    hub: Arc<Hub>,
    settings: ListenerSettings,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> ListenerHandle {
    let (established_tx, established) = watch::channel(0_u64);
    let (exit_tx, exited) = watch::channel(false);
    let exit = ExitSignal(exit_tx);
    let task = tokio::spawn(async move {
        let _exit = exit;
        run(store, hub, settings, shutdown, established_tx).await;
    });
    ListenerHandle {
        task,
        established,
        liveness: Liveness { exited },
    }
}

enum Ending {
    Shutdown,
    Lost,
}

async fn run(
    store: Store,
    hub: Arc<Hub>,
    settings: ListenerSettings,
    shutdown: impl Future<Output = ()>,
    established: watch::Sender<u64>,
) {
    tokio::pin!(shutdown);
    let mut backoff = Backoff::new(settings.initial_backoff, settings.max_backoff);
    let mut connections = 0_u64;
    loop {
        let connected = tokio::select! {
            () = &mut shutdown => return,
            connected = connect(&store) => connected,
        };
        match connected {
            Ok(listener) => {
                let since = Instant::now();
                connections += 1;
                if connections > 1 {
                    emit::listener_reconnected();
                }
                tracing::info!(channel = CHANNEL, "listening for notifications");
                hub.broadcast_resync(ResyncReason::ListenerReconnect);
                established.send_modify(|count| *count += 1);
                let ending = consume(
                    listener,
                    store.pool(),
                    &hub,
                    settings.heartbeat_interval,
                    &mut shutdown,
                )
                .await;
                if let Ending::Shutdown = ending {
                    return;
                }
                backoff.settle(since.elapsed(), settings.stable_after);
            }
            Err(error) => tracing::warn!(%error, "cannot start the notification listener"),
        }
        let delay = jitter::jittered(backoff.next_delay(), BACKOFF_JITTER);
        tracing::warn!(?delay, "reconnecting the notification listener");
        tokio::select! {
            () = &mut shutdown => return,
            () = tokio::time::sleep(delay) => {}
        }
    }
}

async fn connect(store: &Store) -> Result<PgListener, sqlx::Error> {
    let mut listener = PgListener::connect_with(store.pool()).await?;
    listener.eager_reconnect(false);
    listener.listen(CHANNEL).await?;
    Ok(listener)
}

async fn send_heartbeat(pool: PgPool, limit: Duration) {
    let sent = tokio::time::timeout(
        limit,
        sqlx::query(NOTIFY)
            .bind(CHANNEL)
            .bind(HEARTBEAT_NOTIFICATION)
            .execute(&pool),
    )
    .await;
    match sent {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(%error, "cannot send a listener heartbeat"),
        Err(_) => tracing::warn!(?limit, "a listener heartbeat did not finish in time"),
    }
}

enum Step {
    Shutdown,
    Silent,
    Received(Result<Option<PgNotification>, sqlx::Error>),
}

async fn consume(
    mut listener: PgListener,
    pool: &PgPool,
    hub: &Hub,
    heartbeat_interval: Duration,
    shutdown: &mut (impl Future<Output = ()> + Unpin),
) -> Ending {
    let silence_limit = heartbeat_interval.saturating_mul(SILENCE_FACTOR);
    let mut beat = tokio::time::interval(heartbeat_interval);
    beat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut senders = JoinSet::new();
    let mut last_heard = Instant::now();
    loop {
        let step = {
            let receiving = listener.try_recv();
            tokio::pin!(receiving);
            loop {
                tokio::select! {
                    () = &mut *shutdown => break Step::Shutdown,
                    _ = beat.tick() => {
                        senders.spawn(send_heartbeat(pool.clone(), heartbeat_interval));
                    }
                    Some(finished) = senders.join_next(), if !senders.is_empty() => {
                        if let Err(error) = finished {
                            tracing::error!(%error, "a listener heartbeat task failed");
                        }
                    }
                    () = tokio::time::sleep_until(last_heard + silence_limit) => break Step::Silent,
                    received = &mut receiving => break Step::Received(received),
                }
            }
        };
        match step {
            Step::Shutdown => return Ending::Shutdown,
            Step::Silent => {
                tracing::warn!(
                    ?silence_limit,
                    "no notification or heartbeat arrived, the listener connection is presumed dead"
                );
                return Ending::Lost;
            }
            Step::Received(Ok(Some(notification))) => {
                last_heard = Instant::now();
                dispatch(hub, notification.payload());
            }
            Step::Received(Ok(None)) => {
                tracing::warn!("the notification connection was lost");
                return Ending::Lost;
            }
            Step::Received(Err(error)) => {
                tracing::warn!(%error, "the notification listener failed");
                return Ending::Lost;
            }
        }
    }
}

pub(crate) fn dispatch(hub: &Hub, payload: &str) {
    match Notification::decode(payload) {
        Ok(Notification::Heartbeat) => {}
        Ok(Notification::Event { endpoint, event }) => {
            hub.publish(&endpoint, event);
        }
        Err(DecodeError::UnknownType { kind }) => {
            tracing::debug!(kind, "dropped a notification of an unknown type");
        }
        Err(error) => {
            tracing::warn!(%error, bytes = payload.len(), "dropped a malformed notification");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_cap_and_resets() {
        let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        let delays: Vec<Duration> = (0..10).map(|_| backoff.next_delay()).collect();
        assert_eq!(delays[0], Duration::from_millis(100));
        assert_eq!(delays[1], Duration::from_millis(200));
        assert_eq!(delays[2], Duration::from_millis(400));
        assert_eq!(delays[7], MAX_BACKOFF);
        assert_eq!(delays[9], MAX_BACKOFF);
        backoff.reset();
        assert_eq!(backoff.next_delay(), INITIAL_BACKOFF);
    }

    #[test]
    fn backoff_resets_only_after_a_stable_connection() {
        let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
        backoff.next_delay();
        backoff.next_delay();

        backoff.settle(STABLE_AFTER - Duration::from_millis(1), STABLE_AFTER);
        assert_eq!(backoff.next_delay(), Duration::from_millis(400));

        backoff.settle(STABLE_AFTER, STABLE_AFTER);
        assert_eq!(backoff.next_delay(), INITIAL_BACKOFF);
    }
}
