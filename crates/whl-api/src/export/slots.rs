use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_auth::Caller;
use whl_store::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum CallerKey {
    User(Uuid),
    Address(IpAddr),
    Unknown,
}

impl CallerKey {
    fn of(caller: &Caller) -> Self {
        match caller.user() {
            Some(user) => Self::User(user.id),
            None => caller.ip().map_or(Self::Unknown, Self::Address),
        }
    }
}

type Active = Arc<Mutex<HashSet<CallerKey>>>;

#[derive(Clone)]
pub struct ExportSlots {
    replica: Arc<Semaphore>,
    callers: Active,
}

impl ExportSlots {
    pub fn new(replica_limit: u32) -> Self {
        Self {
            replica: Arc::new(Semaphore::new(
                usize::try_from(replica_limit).unwrap_or(usize::MAX),
            )),
            callers: Arc::default(),
        }
    }

    pub fn acquire(&self, caller: &Caller) -> Option<ExportPermit> {
        let key = CallerKey::of(caller);
        if !lock(&self.callers).insert(key.clone()) {
            return None;
        }
        match Arc::clone(&self.replica).try_acquire_owned() {
            Ok(replica) => Some(ExportPermit {
                _replica: replica,
                callers: Arc::clone(&self.callers),
                key,
            }),
            Err(_) => {
                lock(&self.callers).remove(&key);
                None
            }
        }
    }
}

pub struct ExportPermit {
    _replica: OwnedSemaphorePermit,
    callers: Active,
    key: CallerKey,
}

impl Drop for ExportPermit {
    fn drop(&mut self) {
        lock(&self.callers).remove(&self.key);
    }
}

fn lock(callers: &Mutex<HashSet<CallerKey>>) -> MutexGuard<'_, HashSet<CallerKey>> {
    callers.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    fn anonymous(last_octet: u8) -> Caller {
        Caller::Anonymous {
            ip: Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, last_octet))),
        }
    }

    #[test]
    fn a_second_export_from_the_same_caller_is_refused_until_the_first_ends() {
        let slots = ExportSlots::new(2);
        let first = slots.acquire(&anonymous(7));
        assert!(first.is_some());
        assert!(slots.acquire(&anonymous(7)).is_none());
        drop(first);
        assert!(slots.acquire(&anonymous(7)).is_some());
    }

    #[test]
    fn the_replica_limit_refuses_a_new_caller_while_all_slots_are_busy() {
        let slots = ExportSlots::new(1);
        let busy = slots.acquire(&anonymous(7));
        assert!(busy.is_some());
        assert!(slots.acquire(&anonymous(8)).is_none());
    }
}
