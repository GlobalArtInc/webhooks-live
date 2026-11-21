use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};
use whl_telemetry::series::DELAYED_RESPONSES;

pub const DELAY_SKIPPED_TOTAL: &str = "webhooks_live_delay_skipped_total";

#[derive(Debug, Clone)]
pub struct DelaySlots {
    semaphore: Arc<Semaphore>,
    capacity: usize,
}

struct HeldSlot {
    _permit: OwnedSemaphorePermit,
}

impl HeldSlot {
    fn new(permit: OwnedSemaphorePermit) -> Self {
        metrics::gauge!(DELAYED_RESPONSES).increment(1.0);
        Self { _permit: permit }
    }
}

impl Drop for HeldSlot {
    fn drop(&mut self) {
        metrics::gauge!(DELAYED_RESPONSES).decrement(1.0);
    }
}

impl DelaySlots {
    pub fn new(capacity: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(capacity)),
            capacity,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn in_use(&self) -> usize {
        self.capacity
            .saturating_sub(self.semaphore.available_permits())
    }

    pub async fn wait(&self, delay: Duration) {
        if delay.is_zero() {
            return;
        }
        let permit = match Arc::clone(&self.semaphore).try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => {
                metrics::counter!(DELAY_SKIPPED_TOTAL).increment(1);
                return;
            }
            Err(TryAcquireError::Closed) => {
                tracing::error!("the delay slot semaphore is closed");
                return;
            }
        };
        let _slot = HeldSlot::new(permit);
        tokio::time::sleep(delay).await;
    }
}
