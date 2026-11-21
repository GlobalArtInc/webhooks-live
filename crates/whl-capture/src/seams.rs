use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::{Limiters, Verdict, whole_seconds_up};
use whl_telemetry::series;

const SCOPE_CAPTURE_IP: &str = "capture_ip";
const SCOPE_CAPTURE_ENDPOINT: &str = "capture_endpoint";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Allow,
    Limited { retry_after: Duration },
}

pub(crate) fn admit_client(limiters: Option<&Limiters>, client_ip: IpAddr) -> Admission {
    let Some(limiters) = limiters else {
        return Admission::Allow;
    };
    admission(limiters.check_capture_ip(client_ip), SCOPE_CAPTURE_IP)
}

pub(crate) fn admit_endpoint(limiters: Option<&Limiters>, endpoint: &EndpointId) -> Admission {
    let Some(limiters) = limiters else {
        return Admission::Allow;
    };
    admission(
        limiters.check_capture_endpoint(endpoint),
        SCOPE_CAPTURE_ENDPOINT,
    )
}

fn admission(verdict: Verdict, scope: &'static str) -> Admission {
    match verdict {
        Verdict::Allowed { .. } => Admission::Allow,
        Verdict::Limited { retry_after, .. } => {
            metrics::counter!(series::RATE_LIMITED_TOTAL, "scope" => scope).increment(1);
            Admission::Limited {
                retry_after: Duration::from_secs(whole_seconds_up(retry_after)),
            }
        }
    }
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
