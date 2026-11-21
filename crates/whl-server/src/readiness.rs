use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub struct Readiness {
    ready: AtomicBool,
}

impl Readiness {
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }

    pub fn mark_draining(&self) {
        self.ready.store(false, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}
