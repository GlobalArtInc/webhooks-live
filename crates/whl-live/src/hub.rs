use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::broadcast;
use whl_core::endpoint_id::EndpointId;

use crate::event::{Event, ResyncReason};

pub const HUB_CAPACITY: usize = 256;

#[derive(Debug)]
struct Registry {
    senders: HashMap<EndpointId, broadcast::Sender<Arc<Event>>>,
    closing: bool,
}

#[derive(Debug)]
pub struct Hub {
    capacity: usize,
    registry: Mutex<Registry>,
}

#[derive(Debug)]
pub struct Subscription {
    receiver: Option<broadcast::Receiver<Arc<Event>>>,
    hub: Arc<Hub>,
    endpoint: EndpointId,
    closing: bool,
}

impl Hub {
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            capacity: capacity.max(1),
            registry: Mutex::new(Registry {
                senders: HashMap::new(),
                closing: false,
            }),
        })
    }

    pub fn subscribe(self: &Arc<Self>, endpoint: &EndpointId) -> Subscription {
        let mut registry = self.lock();
        let capacity = self.capacity;
        let sender = registry
            .senders
            .entry(endpoint.clone())
            .or_insert_with(|| broadcast::channel(capacity).0);
        let receiver = sender.subscribe();
        Subscription {
            receiver: Some(receiver),
            hub: Arc::clone(self),
            endpoint: endpoint.clone(),
            closing: registry.closing,
        }
    }

    pub fn publish(&self, endpoint: &EndpointId, event: Event) -> bool {
        let registry = self.lock();
        match registry.senders.get(endpoint) {
            Some(sender) => sender.send(Arc::new(event)).is_ok(),
            None => false,
        }
    }

    pub fn broadcast_resync(&self, reason: ResyncReason) {
        self.broadcast(&Arc::new(Event::Resync { reason }));
    }

    pub fn close_streams(&self) {
        let mut registry = self.lock();
        registry.closing = true;
        let bye = Arc::new(Event::Bye);
        for sender in registry.senders.values() {
            let _ = sender.send(Arc::clone(&bye));
        }
    }

    pub fn is_closing(&self) -> bool {
        self.lock().closing
    }

    pub fn endpoint_count(&self) -> usize {
        self.lock().senders.len()
    }

    pub fn subscriber_count(&self, endpoint: &EndpointId) -> usize {
        self.lock()
            .senders
            .get(endpoint)
            .map_or(0, broadcast::Sender::receiver_count)
    }

    fn broadcast(&self, event: &Arc<Event>) {
        let registry = self.lock();
        for sender in registry.senders.values() {
            let _ = sender.send(Arc::clone(event));
        }
    }

    fn release(&self, endpoint: &EndpointId) {
        let mut registry = self.lock();
        let unused = registry
            .senders
            .get(endpoint)
            .is_some_and(|sender| sender.receiver_count() == 0);
        if unused {
            registry.senders.remove(endpoint);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.registry.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Subscription {
    pub fn was_closing_at_subscribe(&self) -> bool {
        self.closing
    }

    pub async fn recv(&mut self) -> Result<Arc<Event>, broadcast::error::RecvError> {
        match self.receiver.as_mut() {
            Some(receiver) => receiver.recv().await,
            None => Err(broadcast::error::RecvError::Closed),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.receiver.take();
        self.hub.release(&self.endpoint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> EndpointId {
        EndpointId::generate().expect("the operating system provides entropy")
    }

    #[test]
    fn discards_events_for_an_endpoint_with_no_subscriber() {
        let hub = Hub::new(4);
        assert!(!hub.publish(&endpoint(), Event::CapturesCleared));
        assert_eq!(hub.endpoint_count(), 0);
    }

    #[test]
    fn removes_the_entry_when_the_last_subscriber_drops() {
        let hub = Hub::new(4);
        let id = endpoint();
        let first = hub.subscribe(&id);
        let second = hub.subscribe(&id);
        assert_eq!(hub.subscriber_count(&id), 2);
        drop(first);
        assert_eq!(hub.endpoint_count(), 1);
        drop(second);
        assert_eq!(hub.endpoint_count(), 0);
    }

    #[tokio::test]
    async fn delivers_to_every_subscriber_of_the_endpoint_only() {
        let hub = Hub::new(4);
        let id = endpoint();
        let mut first = hub.subscribe(&id);
        let mut second = hub.subscribe(&id);
        let mut other = hub.subscribe(&endpoint());

        assert!(hub.publish(&id, Event::CapturesCleared));

        assert_eq!(
            *first.recv().await.expect("first receives"),
            Event::CapturesCleared
        );
        assert_eq!(
            *second.recv().await.expect("second receives"),
            Event::CapturesCleared
        );
        assert!(
            other
                .receiver
                .as_mut()
                .expect("receiver")
                .try_recv()
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_full_buffer_lags_the_receiver_without_blocking_the_publisher() {
        let hub = Hub::new(2);
        let id = endpoint();
        let mut slow = hub.subscribe(&id);
        for _ in 0..5 {
            hub.publish(&id, Event::CapturesCleared);
        }
        assert!(matches!(
            slow.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }

    #[test]
    fn close_marks_later_subscriptions_as_closing() {
        let hub = Hub::new(2);
        let id = endpoint();
        let before = hub.subscribe(&id);
        hub.close_streams();
        let after = hub.subscribe(&id);
        assert!(!before.was_closing_at_subscribe());
        assert!(after.was_closing_at_subscribe());
        assert!(hub.is_closing());
    }
}
