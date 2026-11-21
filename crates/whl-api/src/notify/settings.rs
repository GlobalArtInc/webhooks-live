#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NotifyApiSettings {
    pub enabled: bool,
}

impl NotifyApiSettings {
    pub fn enabled() -> Self {
        Self { enabled: true }
    }
}
