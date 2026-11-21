#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleSettings {
    pub enabled: bool,
}

impl ScheduleSettings {
    pub fn enabled() -> Self {
        Self { enabled: true }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }
}

impl Default for ScheduleSettings {
    fn default() -> Self {
        Self::disabled()
    }
}
