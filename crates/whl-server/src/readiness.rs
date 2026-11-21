use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default)]
pub struct Readiness {
    ready: AtomicBool,
    auth_unavailable: AtomicBool,
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

    pub fn set_auth_unavailable(&self, unavailable: bool) {
        self.auth_unavailable.store(unavailable, Ordering::Release);
    }

    pub fn auth_unavailable(&self) -> bool {
        self.auth_unavailable.load(Ordering::Acquire)
    }
}
