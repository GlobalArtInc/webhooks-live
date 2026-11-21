use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleSettings {
    pub enabled: bool,
    pub max_per_user: usize,
    pub min_interval: Duration,
}

impl ScheduleSettings {
    pub fn new(max_per_user: usize, min_interval: Duration) -> Self {
        Self {
            enabled: true,
            max_per_user,
            min_interval,
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            max_per_user: 0,
            min_interval: Duration::ZERO,
        }
    }
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self::disabled()
    }
}
