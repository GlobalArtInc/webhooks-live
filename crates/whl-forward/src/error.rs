use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RefusalReason {
    #[error("scheme is not http or https")]
    Scheme,
    #[error("url carries user information")]
    Userinfo,
    #[error("url has no host")]
    MissingHost,
    #[error("host name is not allowed")]
    ForbiddenHost,
    #[error("url is not valid")]
    InvalidUrl,
    #[error("url is too long")]
    UrlTooLong,
    #[error("address is in a refused range")]
    RefusedAddress,
    #[error("redirect from https to http")]
    HttpsDowngrade,
}

impl RefusalReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Scheme => "scheme",
            Self::Userinfo => "userinfo",
            Self::MissingHost => "missing_host",
            Self::ForbiddenHost => "forbidden_host",
            Self::InvalidUrl => "invalid_url",
            Self::UrlTooLong => "url_too_long",
            Self::RefusedAddress => "refused_address",
            Self::HttpsDowngrade => "https_downgrade",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ForwardError {
    #[error("target refused: {0}")]
    TargetRefused(RefusalReason),
    #[error("redirect target refused: {0}")]
    RedirectRefused(RefusalReason),
    #[error("dns lookup failed: {0}")]
    DnsFailure(String),
    #[error("connect timeout")]
    ConnectTimeout,
    #[error("hop timeout")]
    HopTimeout,
    #[error("total timeout")]
    TotalTimeout,
    #[error("too many redirects (limit {limit})")]
    TooManyRedirects { limit: u32 },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("invalid client configuration: {0}")]
    Configuration(String),
}

impl ForwardError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TargetRefused(_) => "refused_target",
            Self::RedirectRefused(_) => "refused_redirect",
            Self::DnsFailure(_) => "dns_failure",
            Self::ConnectTimeout => "connect_timeout",
            Self::HopTimeout => "hop_timeout",
            Self::TotalTimeout => "total_timeout",
            Self::TooManyRedirects { .. } => "too_many_redirects",
            Self::Transport(_) => "transport",
            Self::Configuration(_) => "configuration",
        }
    }

    pub const fn refusal(&self) -> Option<RefusalReason> {
        match self {
            Self::TargetRefused(reason) | Self::RedirectRefused(reason) => Some(*reason),
            _ => None,
        }
    }

    pub(crate) fn at_hop(self, redirects_followed: u32) -> Self {
        match self {
            Self::TargetRefused(reason) if redirects_followed > 0 => Self::RedirectRefused(reason),
            other => other,
        }
    }
}
