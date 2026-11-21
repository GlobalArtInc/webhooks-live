use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::http::HeaderMap;
use openidconnect::reqwest;
use whl_store::{SessionLifetime, Store, UserRecord};

use crate::cookies::CookiePolicy;
use crate::error::{AuthError, AuthRejection};
use crate::origin::OriginCheck;
use crate::provider::{self, Provider};
use crate::settings::AuthSettings;
use crate::token::TokenHash;

const INITIAL_DISCOVERY_DELAY: Duration = Duration::from_secs(1);
const MAX_DISCOVERY_DELAY: Duration = Duration::from_secs(60);

struct Inner {
    store: Store,
    settings: AuthSettings,
    http: reqwest::Client,
    provider: OnceLock<Provider>,
    cookies: CookiePolicy,
    origin: OriginCheck,
}

#[derive(Clone)]
pub struct AuthState {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryBackoff {
    next: Duration,
}

impl DiscoveryBackoff {
    pub fn new() -> Self {
        Self {
            next: INITIAL_DISCOVERY_DELAY,
        }
    }

    pub fn next_delay(&mut self) -> Duration {
        let current = self.next;
        self.next = (current * 2).min(MAX_DISCOVERY_DELAY);
        current
    }
}

impl Default for DiscoveryBackoff {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthState {
    pub fn new(store: Store, settings: AuthSettings) -> Result<Self, AuthError> {
        let http = provider::http_client()?;
        let cookies = CookiePolicy::new(settings.secure_cookies());
        if !settings.secure_cookies() {
            tracing::warn!(
                public_origin = settings.public_origin(),
                "WEBHOOKS_LIVE_PUBLIC_URL is http: session cookies are issued without the Secure flag and the __Host- prefix; use this for local development only"
            );
        }
        let origin = OriginCheck::new(settings.public_origin(), cookies.clone());
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                settings,
                http,
                provider: OnceLock::new(),
                cookies,
                origin,
            }),
        })
    }

    pub async fn discover(store: Store, settings: AuthSettings) -> Result<Self, AuthError> {
        let state = Self::new(store, settings)?;
        state.establish().await?;
        Ok(state)
    }

    pub async fn establish(&self) -> Result<(), AuthError> {
        if self.inner.provider.get().is_some() {
            return Ok(());
        }
        let provider =
            Provider::discover(self.inner.http.clone(), self.inner.settings.clone()).await?;
        if self.inner.provider.set(provider).is_err() {
            tracing::debug!("another task finished the OIDC discovery first");
        }
        Ok(())
    }

    pub fn is_available(&self) -> bool {
        self.inner.provider.get().is_some()
    }

    pub fn origin_check(&self) -> OriginCheck {
        self.inner.origin.clone()
    }

    pub fn cookies(&self) -> &CookiePolicy {
        &self.inner.cookies
    }

    pub fn settings(&self) -> &AuthSettings {
        &self.inner.settings
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn provider(&self) -> Result<&Provider, AuthRejection> {
        self.inner
            .provider
            .get()
            .ok_or(AuthRejection::AuthUnavailable)
    }

    pub(crate) fn lifetime(&self) -> SessionLifetime {
        SessionLifetime {
            idle: self.inner.settings.limits().idle,
            touch_after: self.inner.settings.touch_after,
        }
    }

    pub(crate) fn session_hash(&self, headers: &HeaderMap) -> Option<TokenHash> {
        self.inner
            .cookies
            .read_session(headers)
            .map(|token| token.digest())
    }

    pub async fn resolve_user(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<UserRecord>, AuthRejection> {
        let Some(hash) = self.session_hash(headers) else {
            return Ok(None);
        };
        let user = self
            .inner
            .store
            .session_user(hash.unpack(), self.lifetime())
            .await?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_backoff_doubles_up_to_the_ceiling() {
        let mut backoff = DiscoveryBackoff::new();
        let delays: Vec<u64> = (0..9).map(|_| backoff.next_delay().as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }
}
