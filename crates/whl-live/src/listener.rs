use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use whl_store::Store;

use crate::event::{DecodeError, Event, ResyncReason};
use crate::hub::Hub;

pub const CHANNEL: &str = "whl_events";
pub const INITIAL_BACKOFF: Duration = Duration::from_millis(100);
pub const MAX_BACKOFF: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub enum ListenerError {
    #[error("the listener task has stopped")]
    Stopped,
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
}

#[derive(Debug)]
pub struct ListenerHandle {
    task: JoinHandle<()>,
    established: watch::Receiver<u64>,
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

    pub async fn join(self) {
        if let Err(error) = self.task.await {
            tracing::error!(%error, "the listener task ended abnormally");
        }
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

pub fn spawn_listener(
    store: Store,
    hub: Arc<Hub>,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> ListenerHandle {
    let (established_tx, established) = watch::channel(0_u64);
    let task = tokio::spawn(run(store, hub, shutdown, established_tx));
    ListenerHandle { task, established }
}

enum Ending {
    Shutdown,
    Lost,
}

async fn run(
    store: Store,
    hub: Arc<Hub>,
    shutdown: impl Future<Output = ()>,
    established: watch::Sender<u64>,
) {
    tokio::pin!(shutdown);
    let mut backoff = Backoff::new(INITIAL_BACKOFF, MAX_BACKOFF);
    loop {
        let connected = tokio::select! {
            () = &mut shutdown => return,
            connected = connect(&store) => connected,
        };
        match connected {
            Ok(listener) => {
                backoff.reset();
                tracing::info!(channel = CHANNEL, "listening for notifications");
                hub.broadcast_resync(ResyncReason::ListenerReconnect);
                established.send_modify(|count| *count += 1);
                if let Ending::Shutdown = consume(listener, &hub, &mut shutdown).await {
                    return;
                }
            }
            Err(error) => tracing::warn!(%error, "cannot start the notification listener"),
        }
        let delay = backoff.next_delay();
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

async fn consume(
    mut listener: PgListener,
    hub: &Hub,
    shutdown: &mut (impl Future<Output = ()> + Unpin),
) -> Ending {
    loop {
        let received = tokio::select! {
            () = &mut *shutdown => return Ending::Shutdown,
            received = listener.try_recv() => received,
        };
        match received {
            Ok(Some(notification)) => dispatch(hub, notification.payload()),
            Ok(None) => {
                tracing::warn!("the notification connection was lost");
                return Ending::Lost;
            }
            Err(error) => {
                tracing::warn!(%error, "the notification listener failed");
                return Ending::Lost;
            }
        }
    }
}

pub(crate) fn dispatch(hub: &Hub, payload: &str) {
    match Event::decode(payload) {
        Ok((endpoint, event)) => {
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
}
