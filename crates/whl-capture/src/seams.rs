use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Allow,
    Limited { retry_after: Duration },
}

pub(crate) fn admit_client(_client_ip: IpAddr) -> Admission {
    Admission::Allow
}

#[derive(Debug, Clone, Default)]
pub struct StorageFull {
    flag: Arc<AtomicBool>,
}

impl StorageFull {
    pub fn is_full(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    pub fn set(&self, full: bool) {
        self.flag.store(full, Ordering::Relaxed);
    }
}
