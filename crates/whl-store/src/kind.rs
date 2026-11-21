#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Http,
    Email,
    Dns,
    Schedule,
}

impl CaptureKind {
    pub const ALL: [Self; 4] = [Self::Http, Self::Email, Self::Dns, Self::Schedule];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Email => "email",
            Self::Dns => "dns",
            Self::Schedule => "schedule",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}
