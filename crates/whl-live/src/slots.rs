use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub const INBOX_STREAMS_PER_PRINCIPAL: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SlotKey {
    User(String),
    Address(IpAddr),
}

#[derive(Debug, Default)]
pub struct InboxSlots {
    held: Mutex<HashMap<SlotKey, usize>>,
}

#[derive(Debug)]
pub struct SlotGuard {
    slots: Arc<InboxSlots>,
    keys: Vec<SlotKey>,
}

impl InboxSlots {
    pub fn acquire(
        self: &Arc<Self>,
        user: Option<&str>,
        address: Option<IpAddr>,
        max: usize,
    ) -> Option<SlotGuard> {
        let keys: Vec<SlotKey> = user
            .map(|user| SlotKey::User(user.to_owned()))
            .into_iter()
            .chain(address.map(SlotKey::Address))
            .collect();
        let mut held = self.lock();
        if keys
            .iter()
            .any(|key| held.get(key).copied().unwrap_or(0) >= max)
        {
            return None;
        }
        for key in &keys {
            *held.entry(key.clone()).or_insert(0) += 1;
        }
        Some(SlotGuard {
            slots: Arc::clone(self),
            keys,
        })
    }

    pub fn open_streams(&self) -> usize {
        self.lock().values().sum()
    }

    fn release(&self, keys: &[SlotKey]) {
        let mut held = self.lock();
        for key in keys {
            let emptied = match held.get_mut(key) {
                Some(count) => {
                    *count = count.saturating_sub(1);
                    *count == 0
                }
                None => false,
            };
            if emptied {
                held.remove(key);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<SlotKey, usize>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        self.slots.release(&self.keys);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    #[test]
    fn a_principal_over_the_cap_is_refused_and_dropping_a_guard_frees_a_slot() {
        let slots = Arc::new(InboxSlots::default());
        let first = slots.acquire(Some("u"), None, 2).expect("first");
        let _second = slots.acquire(Some("u"), None, 2).expect("second");
        assert!(slots.acquire(Some("u"), None, 2).is_none());
        assert!(slots.acquire(Some("other"), None, 2).is_some());
        drop(first);
        assert!(slots.acquire(Some("u"), None, 2).is_some());
    }

    #[test]
    fn the_address_cap_holds_across_users_and_a_refusal_takes_no_slot() {
        let slots = Arc::new(InboxSlots::default());
        let _first = slots
            .acquire(Some("a"), Some(address(1)), 1)
            .expect("first");
        assert!(slots.acquire(Some("b"), Some(address(1)), 1).is_none());
        assert_eq!(slots.open_streams(), 2);
        assert!(slots.acquire(Some("b"), Some(address(2)), 1).is_some());
    }

    #[test]
    fn released_entries_leave_the_map() {
        let slots = Arc::new(InboxSlots::default());
        drop(slots.acquire(Some("u"), Some(address(1)), 4));
        assert_eq!(slots.open_streams(), 0);
    }
}
