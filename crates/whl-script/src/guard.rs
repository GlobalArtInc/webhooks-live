use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use rhai::{Dynamic, EvalAltResult, Position};

use crate::alloc::memory_exceeded;
use crate::env::MonotonicClock;
use crate::limits::DEADLINE_CHECK_INTERVAL;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Abort {
    Time,
    Memory,
    Cancelled,
    HostBudget,
}

impl Abort {
    fn code(self) -> u8 {
        match self {
            Self::Time => 1,
            Self::Memory => 2,
            Self::Cancelled => 3,
            Self::HostBudget => 4,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Time),
            2 => Some(Self::Memory),
            3 => Some(Self::Cancelled),
            4 => Some(Self::HostBudget),
            _ => None,
        }
    }
}

pub(crate) struct RunGuard {
    clock: Arc<dyn MonotonicClock>,
    deadline: Duration,
    cancel: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    cause: AtomicU8,
    operations: AtomicU64,
    host_bytes: AtomicU64,
    max_host_bytes: u64,
}

impl RunGuard {
    pub(crate) fn new(
        clock: Arc<dyn MonotonicClock>,
        deadline: Duration,
        cancel: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        max_host_bytes: u64,
    ) -> Self {
        Self {
            clock,
            deadline,
            cancel,
            stop,
            cause: AtomicU8::new(0),
            operations: AtomicU64::new(0),
            host_bytes: AtomicU64::new(0),
            max_host_bytes,
        }
    }

    fn record(&self, abort: Abort) -> Abort {
        let _ = self
            .cause
            .compare_exchange(0, abort.code(), Ordering::SeqCst, Ordering::SeqCst);
        self.cause().unwrap_or(abort)
    }

    pub(crate) fn cause(&self) -> Option<Abort> {
        Abort::from_code(self.cause.load(Ordering::SeqCst))
    }

    pub(crate) fn operations(&self) -> u64 {
        self.operations.load(Ordering::SeqCst)
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.clock.elapsed()
    }

    fn flags(&self) -> Option<Abort> {
        if memory_exceeded() {
            return Some(Abort::Memory);
        }
        if self.cancel.load(Ordering::SeqCst) || self.stop.load(Ordering::SeqCst) {
            return Some(Abort::Cancelled);
        }
        None
    }

    pub(crate) fn poll(&self) -> Option<Abort> {
        if let Some(cause) = self.cause() {
            return Some(cause);
        }
        if let Some(abort) = self.flags() {
            return Some(self.record(abort));
        }
        if self.clock.elapsed() >= self.deadline {
            return Some(self.record(Abort::Time));
        }
        None
    }

    pub(crate) fn progress(&self, operations: u64) -> Option<Dynamic> {
        self.operations.store(operations, Ordering::SeqCst);
        if let Some(cause) = self.cause() {
            return Some(token(cause));
        }
        if let Some(abort) = self.flags() {
            return Some(token(self.record(abort)));
        }
        if operations.is_multiple_of(DEADLINE_CHECK_INTERVAL)
            && self.clock.elapsed() >= self.deadline
        {
            return Some(token(self.record(Abort::Time)));
        }
        None
    }

    pub(crate) fn charge(&self, bytes: usize) -> Result<(), Box<EvalAltResult>> {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let total = self
            .host_bytes
            .fetch_add(bytes, Ordering::SeqCst)
            .saturating_add(bytes);
        if total > self.max_host_bytes {
            return Err(terminated(self.record(Abort::HostBudget)));
        }
        match self.poll() {
            Some(abort) => Err(terminated(abort)),
            None => Ok(()),
        }
    }
}

fn token(abort: Abort) -> Dynamic {
    Dynamic::from(i64::from(abort.code()))
}

pub(crate) fn terminated(abort: Abort) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorTerminated(token(abort), Position::NONE))
}
