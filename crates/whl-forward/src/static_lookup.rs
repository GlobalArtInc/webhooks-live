use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::lookup::{LookupFuture, LookupHost};

#[derive(Debug, Default)]
pub struct StaticLookup {
    answers: HashMap<String, Vec<IpAddr>>,
    delay: Duration,
    calls: AtomicUsize,
}

impl StaticLookup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_answer(mut self, host: &str, addresses: &[IpAddr]) -> Self {
        self.answers
            .insert(host.to_ascii_lowercase(), addresses.to_vec());
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl LookupHost for StaticLookup {
    fn lookup<'a>(&'a self, host: &'a str) -> LookupFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            self.answers
                .get(&host.to_ascii_lowercase())
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such host"))
        })
    }
}
