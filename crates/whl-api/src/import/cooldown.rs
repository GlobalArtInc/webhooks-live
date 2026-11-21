use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use whl_store::Uuid;

#[derive(Debug, Clone)]
pub struct ImportCooldown {
    window: Duration,
    finished: Arc<Mutex<HashMap<Uuid, Instant>>>,
}

impl ImportCooldown {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            finished: Arc::default(),
        }
    }

    pub fn check(&self, user: Uuid) -> Result<(), Duration> {
        let now = Instant::now();
        let mut finished = lock(&self.finished);
        finished.retain(|_, at| now.saturating_duration_since(*at) < self.window);
        match finished.get(&user) {
            Some(at) => Err(self
                .window
                .saturating_sub(now.saturating_duration_since(*at))),
            None => Ok(()),
        }
    }

    pub fn finish(&self, user: Uuid) {
        if self.window.is_zero() {
            return;
        }
        lock(&self.finished).insert(user, Instant::now());
    }
}

fn lock(map: &Mutex<HashMap<Uuid, Instant>>) -> MutexGuard<'_, HashMap<Uuid, Instant>> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_user_is_refused_until_the_window_after_their_last_import_has_passed() {
        let cooldown = ImportCooldown::new(Duration::from_secs(60));
        let user = Uuid::from_u128(1);
        assert!(cooldown.check(user).is_ok());
        cooldown.finish(user);
        let remaining = cooldown.check(user).expect_err("the user is cooling down");
        assert!(remaining > Duration::from_secs(50));
        assert!(cooldown.check(Uuid::from_u128(2)).is_ok());
    }

    #[test]
    fn a_zero_window_never_refuses() {
        let cooldown = ImportCooldown::new(Duration::ZERO);
        let user = Uuid::from_u128(1);
        cooldown.finish(user);
        assert!(cooldown.check(user).is_ok());
    }
}
