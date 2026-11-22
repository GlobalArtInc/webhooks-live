use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub const PREVIEW_SLOTS: usize = 8;

#[derive(Clone)]
pub(crate) struct PreviewSlots {
    replica: Arc<Semaphore>,
}

impl PreviewSlots {
    pub(crate) fn new(slots: usize) -> Self {
        Self {
            replica: Arc::new(Semaphore::new(slots)),
        }
    }

    pub(crate) fn acquire(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.replica).try_acquire_owned().ok()
    }
}

impl Default for PreviewSlots {
    fn default() -> Self {
        Self::new(PREVIEW_SLOTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_frees_when_its_permit_drops_and_none_is_left_while_all_are_taken() {
        let slots = PreviewSlots::new(1);
        let held = slots.acquire();
        assert!(held.is_some());
        assert!(slots.acquire().is_none());
        drop(held);
        assert!(slots.acquire().is_some());
    }
}
