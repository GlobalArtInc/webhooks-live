#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepClass {
    Response,
    Compute,
    State,
    Fetch,
    SideEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Respond,
    SetHeader,
    RemoveHeader,
    Delay,
    Stop,
    Extract,
    SetVar,
    Transform,
    SetStore,
    Increment,
    Script,
    Http,
    Forward,
    Notify,
    StoreS3,
}

impl StepKind {
    pub const ALL: [Self; 15] = [
        Self::Respond,
        Self::SetHeader,
        Self::RemoveHeader,
        Self::Delay,
        Self::Stop,
        Self::Extract,
        Self::SetVar,
        Self::Transform,
        Self::SetStore,
        Self::Increment,
        Self::Script,
        Self::Http,
        Self::Forward,
        Self::Notify,
        Self::StoreS3,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Respond => "respond",
            Self::SetHeader => "set_header",
            Self::RemoveHeader => "remove_header",
            Self::Delay => "delay",
            Self::Stop => "stop",
            Self::Extract => "extract",
            Self::SetVar => "set_var",
            Self::Transform => "transform",
            Self::SetStore => "set_store",
            Self::Increment => "increment",
            Self::Script => "script",
            Self::Http => "http",
            Self::Forward => "forward",
            Self::Notify => "notify",
            Self::StoreS3 => "store_s3",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    pub fn class(self) -> StepClass {
        match self {
            Self::Respond | Self::SetHeader | Self::RemoveHeader | Self::Delay | Self::Stop => {
                StepClass::Response
            }
            Self::Extract | Self::SetVar | Self::Transform | Self::Script => StepClass::Compute,
            Self::SetStore | Self::Increment => StepClass::State,
            Self::Http => StepClass::Fetch,
            Self::Forward | Self::Notify | Self::StoreS3 => StepClass::SideEffect,
        }
    }

    pub(crate) fn name_is_field(self) -> bool {
        matches!(self, Self::SetHeader | Self::RemoveHeader | Self::SetStore)
    }

    pub(crate) fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Respond => &["status", "content_type", "headers", "body"],
            Self::SetHeader => &["name", "value"],
            Self::RemoveHeader => &["name"],
            Self::Delay => &["ms"],
            Self::Stop => &[],
            Self::Extract => &["from", "json_path", "regex", "group", "into"],
            Self::SetVar => &["into", "value"],
            Self::Transform => &["into", "template"],
            Self::SetStore => &["name", "value"],
            Self::Increment => &["counter", "by"],
            Self::Script => &["source"],
            Self::Http => &[
                "method",
                "url",
                "headers",
                "body",
                "timeout_ms",
                "follow_redirects",
            ],
            Self::Forward => &[
                "forward_rule_id",
                "target_url",
                "method",
                "content_type",
                "header_allowlist",
                "body_from",
            ],
            Self::Notify => &["channel_id", "message"],
            Self::StoreS3 => &[
                "bucket_url",
                "prefix",
                "region",
                "access_key_secret",
                "secret_key_secret",
                "content",
            ],
        }
    }
}

pub fn response_step_kinds() -> Vec<&'static str> {
    StepKind::ALL
        .into_iter()
        .filter(|kind| kind.class() == StepClass::Response)
        .map(StepKind::as_str)
        .collect()
}
