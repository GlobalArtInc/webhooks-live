use std::sync::Arc;

use axum::http::HeaderMap;
use whl_store::{SessionLifetime, Store, UserRecord};

use crate::cookies::CookiePolicy;
use crate::error::{AuthError, AuthRejection};
use crate::origin::OriginCheck;
use crate::provider::Provider;
use crate::settings::AuthSettings;
use crate::token::TokenHash;

struct Inner {
    store: Store,
    settings: AuthSettings,
    provider: Provider,
    cookies: CookiePolicy,
    origin: OriginCheck,
}

#[derive(Clone)]
pub struct AuthState {
    inner: Arc<Inner>,
}

impl AuthState {
    pub async fn discover(store: Store, settings: AuthSettings) -> Result<Self, AuthError> {
        let provider = Provider::discover(settings.clone()).await?;
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
                provider,
                cookies,
                origin,
            }),
        })
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

    pub(crate) fn provider(&self) -> &Provider {
        &self.inner.provider
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
