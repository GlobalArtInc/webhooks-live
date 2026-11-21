use std::net::IpAddr;
use std::time::Duration;

use openidconnect::{ClientId, ClientSecret, IssuerUrl, RedirectUrl, Scope};
use url::{Host, Url};
use whl_core::config::{AuthMode, Config};

use crate::error::AuthError;

const LOGIN_TTL: Duration = Duration::from_secs(600);
const MAX_TOUCH_INTERVAL: Duration = Duration::from_secs(60);
const CALLBACK_PATH: &str = "/auth/callback";

#[derive(Debug, Clone, Copy)]
pub struct OidcClient<'a> {
    pub issuer: &'a Url,
    pub client_id: &'a str,
    pub client_secret: &'a str,
    pub scopes: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    pub idle: Duration,
    pub max: Duration,
}

#[derive(Debug, Clone)]
pub struct AuthSettings {
    pub(crate) issuer: IssuerUrl,
    pub(crate) client_id: ClientId,
    pub(crate) client_secret: ClientSecret,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) redirect_url: RedirectUrl,
    pub(crate) public_origin: String,
    pub(crate) secure_cookies: bool,
    pub(crate) limits: SessionLimits,
    pub(crate) touch_after: Duration,
}

impl AuthSettings {
    pub fn from_config(config: &Config) -> Result<Option<Self>, AuthError> {
        if config.auth.mode != AuthMode::Sso {
            return Ok(None);
        }
        let oidc = &config.auth.oidc;
        let issuer = oidc.issuer.as_ref().ok_or(AuthError::MissingConfig {
            key: "WEBHOOKS_LIVE_OIDC_ISSUER",
        })?;
        let client_id = oidc.client_id.as_deref().ok_or(AuthError::MissingConfig {
            key: "WEBHOOKS_LIVE_OIDC_CLIENT_ID",
        })?;
        let client_secret = oidc
            .client_secret
            .as_ref()
            .ok_or(AuthError::MissingConfig {
                key: "WEBHOOKS_LIVE_OIDC_CLIENT_SECRET",
            })?;
        let settings = Self::new(
            OidcClient {
                issuer,
                client_id,
                client_secret: client_secret.unpack(),
                scopes: &oidc.scopes,
            },
            &config.public_url,
            SessionLimits {
                idle: config.auth.session_idle,
                max: config.auth.session_max,
            },
        )?;
        Ok(Some(settings))
    }

    pub fn new(
        oidc: OidcClient<'_>,
        public_url: &Url,
        limits: SessionLimits,
    ) -> Result<Self, AuthError> {
        if !issuer_transport_is_safe(oidc.issuer) {
            return Err(AuthError::InsecureIssuer {
                issuer: oidc.issuer.to_string(),
            });
        }
        if oidc.client_id.is_empty() {
            return Err(AuthError::InvalidConfig {
                key: "WEBHOOKS_LIVE_OIDC_CLIENT_ID",
                reason: "it is empty".to_owned(),
            });
        }
        if oidc.client_secret.is_empty() {
            return Err(AuthError::InvalidConfig {
                key: "WEBHOOKS_LIVE_OIDC_CLIENT_SECRET",
                reason: "it is empty".to_owned(),
            });
        }
        if limits.idle.is_zero() || limits.max.is_zero() {
            return Err(AuthError::InvalidConfig {
                key: "WEBHOOKS_LIVE_SESSION_IDLE and WEBHOOKS_LIVE_SESSION_MAX",
                reason: "both must be longer than zero".to_owned(),
            });
        }
        let secure_cookies = match public_url.scheme() {
            "https" => true,
            "http" => false,
            other => {
                return Err(AuthError::InvalidConfig {
                    key: "WEBHOOKS_LIVE_PUBLIC_URL",
                    reason: format!("scheme {other:?} is neither http nor https"),
                });
            }
        };
        if public_url.host().is_none() {
            return Err(AuthError::InvalidConfig {
                key: "WEBHOOKS_LIVE_PUBLIC_URL",
                reason: "it has no host".to_owned(),
            });
        }
        let public_origin = public_url.origin().ascii_serialization();
        let redirect_url =
            RedirectUrl::new(format!("{public_origin}{CALLBACK_PATH}")).map_err(|error| {
                AuthError::InvalidConfig {
                    key: "WEBHOOKS_LIVE_PUBLIC_URL",
                    reason: error.to_string(),
                }
            })?;
        let issuer = IssuerUrl::from_url(oidc.issuer.clone());
        Ok(Self {
            issuer,
            client_id: ClientId::new(oidc.client_id.to_owned()),
            client_secret: ClientSecret::new(oidc.client_secret.to_owned()),
            scopes: additional_scopes(oidc.scopes),
            redirect_url,
            public_origin,
            secure_cookies,
            limits,
            touch_after: (limits.idle / 4).min(MAX_TOUCH_INTERVAL),
        })
    }

    pub fn login_ttl(&self) -> Duration {
        LOGIN_TTL
    }

    pub fn secure_cookies(&self) -> bool {
        self.secure_cookies
    }

    pub fn public_origin(&self) -> &str {
        &self.public_origin
    }

    pub fn limits(&self) -> SessionLimits {
        self.limits
    }
}

fn additional_scopes(configured: &[String]) -> Vec<Scope> {
    configured
        .iter()
        .filter(|scope| scope.as_str() != "openid")
        .map(|scope| Scope::new(scope.clone()))
        .collect()
}

fn issuer_transport_is_safe(issuer: &Url) -> bool {
    match issuer.scheme() {
        "https" => true,
        "http" => match issuer.host() {
            Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
            Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
            None => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("test URL parses")
    }

    fn settings(issuer: &str, public: &str) -> Result<AuthSettings, AuthError> {
        let issuer = url(issuer);
        AuthSettings::new(
            OidcClient {
                issuer: &issuer,
                client_id: "client",
                client_secret: "secret",
                scopes: &["profile".to_owned()],
            },
            &url(public),
            SessionLimits {
                idle: Duration::from_secs(3600),
                max: Duration::from_secs(7200),
            },
        )
    }

    #[test]
    fn https_public_url_gives_secure_cookies_and_a_callback_under_its_origin() {
        let settings = settings("https://sso.example", "https://whl.example/").expect("valid");
        assert!(settings.secure_cookies());
        assert_eq!(settings.public_origin(), "https://whl.example");
        assert_eq!(
            settings.redirect_url.as_str(),
            "https://whl.example/auth/callback"
        );
    }

    #[test]
    fn http_public_url_drops_secure_cookies() {
        let settings = settings("https://sso.example", "http://127.0.0.1:8080").expect("valid");
        assert!(!settings.secure_cookies());
        assert_eq!(settings.public_origin(), "http://127.0.0.1:8080");
    }

    #[test]
    fn issuer_must_be_https_or_loopback_http() {
        assert!(settings("https://sso.example", "https://whl.example").is_ok());
        assert!(settings("http://127.0.0.1:9000", "https://whl.example").is_ok());
        assert!(settings("http://localhost:9000", "https://whl.example").is_ok());
        assert!(matches!(
            settings("http://sso.example", "https://whl.example"),
            Err(AuthError::InsecureIssuer { .. })
        ));
        assert!(matches!(
            settings("ftp://sso.example", "https://whl.example"),
            Err(AuthError::InsecureIssuer { .. })
        ));
    }

    #[test]
    fn the_openid_scope_is_left_to_the_flow_and_never_repeated() {
        let scopes = additional_scopes(&["email".to_owned(), "openid".to_owned()]);
        let names: Vec<&str> = scopes.iter().map(|scope| scope.as_str()).collect();
        assert_eq!(names, ["email"]);
    }

    #[test]
    fn touch_interval_never_exceeds_a_quarter_of_the_idle_timeout() {
        let settings = settings("https://sso.example", "https://whl.example").expect("valid");
        assert_eq!(settings.touch_after, Duration::from_secs(60));
        let issuer = url("https://sso.example");
        let short = AuthSettings::new(
            OidcClient {
                issuer: &issuer,
                client_id: "client",
                client_secret: "secret",
                scopes: &[],
            },
            &url("https://whl.example"),
            SessionLimits {
                idle: Duration::from_secs(8),
                max: Duration::from_secs(60),
            },
        )
        .expect("valid");
        assert_eq!(short.touch_after, Duration::from_secs(2));
    }
}
