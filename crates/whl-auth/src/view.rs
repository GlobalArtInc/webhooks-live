use std::fmt;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::random_bytes;
use whl_core::limiter::{ClientKey, DEFAULT_MAX_KEYS, KeyedLimiter, Verdict, whole_seconds_up};
use whl_core::rate::{RatePeriod, RateSpec};
use whl_store::{Store, ViewPasswordHash, ViewStanding, ViewStoreError};
use whl_telemetry::series;

use crate::error::AuthRejection;
use crate::failures::{FailureCounter, FailureSpec};
use crate::grant::{GRANT_LIFETIME, ViewGrant};
use crate::password::{PasswordError, ViewPassword, ViewPasswords, shape_of};

const CLOSED_RETRY: Duration = Duration::from_secs(60);
const UNPROTECTED_DELAY_MS: u64 = 35;
const UNPROTECTED_JITTER_MS: u64 = 12;
const SCOPE_UNLOCK_IP: &str = "unlock_ip";
const SCOPE_UNLOCK_CLIENT: &str = "unlock_client";
const SCOPE_UNLOCK_ENDPOINT: &str = "unlock_endpoint";
const SCOPE_UNLOCK_BUDGET: &str = "unlock_budget";
const SCOPE_PASSWORD_SET_IP: &str = "password_set_ip";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewLimitSettings {
    pub unlock_ip: RateSpec,
    pub unlock_budget: RateSpec,
    pub password_set_ip: RateSpec,
    pub client_failures: FailureSpec,
    pub endpoint_failures: FailureSpec,
    pub max_keys: usize,
}

impl Default for ViewLimitSettings {
    fn default() -> Self {
        Self {
            unlock_ip: RateSpec {
                count: 20,
                period: RatePeriod::Minute,
                burst: 20,
            },
            unlock_budget: RateSpec {
                count: 300,
                period: RatePeriod::Minute,
                burst: 60,
            },
            password_set_ip: RateSpec {
                count: 10,
                period: RatePeriod::Minute,
                burst: 10,
            },
            client_failures: FailureSpec {
                max_failures: 5,
                window: Duration::from_secs(60),
            },
            endpoint_failures: FailureSpec {
                max_failures: 200,
                window: Duration::from_secs(60),
            },
            max_keys: DEFAULT_MAX_KEYS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AttemptKey {
    endpoint: EndpointId,
    client: ClientKey,
}

struct Limits {
    unlock_ip: Option<KeyedLimiter<ClientKey>>,
    unlock_budget: Option<KeyedLimiter<()>>,
    password_set_ip: Option<KeyedLimiter<ClientKey>>,
    client_failures: FailureCounter<AttemptKey>,
    endpoint_failures: FailureCounter<EndpointId>,
    started: Instant,
    last_cleanup: AtomicU64,
}

fn limiter<K: Hash + Eq + Clone>(
    spec: RateSpec,
    max_keys: usize,
    strict: bool,
    name: &'static str,
) -> Option<KeyedLimiter<K>> {
    let built = if strict {
        KeyedLimiter::new_strict(spec, max_keys)
    } else {
        KeyedLimiter::new(spec, max_keys)
    };
    match built {
        Ok(limiter) => Some(limiter),
        Err(error) => {
            tracing::error!(%error, limiter = name, "a view password limiter is unusable, so every attempt it guards is refused");
            None
        }
    }
}

fn verdict<K: Hash + Eq + Clone>(limiter: &Option<KeyedLimiter<K>>, key: &K) -> Verdict {
    match limiter {
        Some(limiter) => limiter.check(key),
        None => Verdict::Limited {
            limit: 0,
            retry_after: CLOSED_RETRY,
        },
    }
}

fn refused(retry_after: Duration, scope: &'static str) -> AuthRejection {
    metrics::counter!(series::RATE_LIMITED_TOTAL, "scope" => scope).increment(1);
    AuthRejection::ViewRateLimited {
        retry_after_secs: whole_seconds_up(retry_after),
    }
}

impl Limits {
    fn new(settings: &ViewLimitSettings) -> Self {
        Self {
            unlock_ip: limiter(settings.unlock_ip, settings.max_keys, true, SCOPE_UNLOCK_IP),
            unlock_budget: limiter(settings.unlock_budget, 1, false, SCOPE_UNLOCK_BUDGET),
            password_set_ip: limiter(
                settings.password_set_ip,
                settings.max_keys,
                true,
                SCOPE_PASSWORD_SET_IP,
            ),
            client_failures: FailureCounter::new(settings.client_failures, settings.max_keys),
            endpoint_failures: FailureCounter::new(settings.endpoint_failures, settings.max_keys),
            started: Instant::now(),
            last_cleanup: AtomicU64::new(0),
        }
    }

    fn tidy(&self) {
        let second = self.started.elapsed().as_secs();
        let last = self.last_cleanup.load(Ordering::Relaxed);
        if second <= last
            || self
                .last_cleanup
                .compare_exchange(last, second, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        if let Some(limiter) = &self.unlock_ip {
            limiter.cleanup();
        }
        if let Some(limiter) = &self.password_set_ip {
            limiter.cleanup();
        }
        self.client_failures.sweep();
        self.endpoint_failures.sweep();
    }

    fn charge<K: Hash + Eq + Clone>(
        limiter: &Option<KeyedLimiter<K>>,
        key: &K,
        scope: &'static str,
    ) -> Result<(), AuthRejection> {
        match verdict(limiter, key) {
            Verdict::Allowed { .. } => Ok(()),
            Verdict::Limited { retry_after, .. } => Err(refused(retry_after, scope)),
        }
    }

    fn ensure_not_blocked(
        &self,
        attempt: &AttemptKey,
        endpoint: &EndpointId,
    ) -> Result<(), AuthRejection> {
        if let Some(wait) = self.client_failures.blocked_for(attempt) {
            return Err(refused(wait, SCOPE_UNLOCK_CLIENT));
        }
        if let Some(wait) = self.endpoint_failures.blocked_for(endpoint) {
            return Err(refused(wait, SCOPE_UNLOCK_ENDPOINT));
        }
        Ok(())
    }

    fn record_failure(&self, attempt: &AttemptKey, endpoint: &EndpointId) {
        self.client_failures.record(attempt);
        self.endpoint_failures.record(endpoint);
    }
}

fn unprotected_delay() -> Duration {
    let jitter = random_bytes::<1>().map_or(0, |bytes| u64::from(bytes[0]) % UNPROTECTED_JITTER_MS);
    Duration::from_millis(UNPROTECTED_DELAY_MS + jitter)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedGrant {
    pub grant: ViewGrant,
    pub expires_at: OffsetDateTime,
}

struct Inner {
    store: Store,
    retention: Duration,
    passwords: ViewPasswords,
    limits: Limits,
    lifetime: Duration,
}

#[derive(Clone)]
pub struct ViewAccess {
    inner: Arc<Inner>,
}

impl fmt::Debug for ViewAccess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewAccess").finish_non_exhaustive()
    }
}

impl From<ViewStoreError> for AuthRejection {
    fn from(error: ViewStoreError) -> Self {
        if error.is_unavailable() {
            Self::StoreUnavailable
        } else {
            tracing::error!(%error, "view password storage failed");
            Self::Internal
        }
    }
}

impl From<PasswordError> for AuthRejection {
    fn from(error: PasswordError) -> Self {
        match error {
            PasswordError::Busy => Self::PasswordBusy,
            other => {
                tracing::error!(error = %other, "password hashing failed");
                Self::Internal
            }
        }
    }
}

impl ViewAccess {
    pub fn new(store: Store, retention: Duration) -> Self {
        Self::with_settings(
            store,
            retention,
            ViewPasswords::default(),
            &ViewLimitSettings::default(),
        )
    }

    pub fn with_settings(
        store: Store,
        retention: Duration,
        passwords: ViewPasswords,
        limits: &ViewLimitSettings,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                retention,
                passwords,
                limits: Limits::new(limits),
                lifetime: GRANT_LIFETIME,
            }),
        }
    }

    pub async fn standing(
        &self,
        endpoint: &EndpointId,
        grant: Option<&ViewGrant>,
    ) -> Result<ViewStanding, AuthRejection> {
        let digest = grant.map(ViewGrant::digest);
        Ok(self
            .inner
            .store
            .view_standing(
                endpoint,
                digest.as_ref().map(|hash| hash.unpack()),
                self.inner.retention,
            )
            .await?)
    }

    pub async fn require(
        &self,
        endpoint: &EndpointId,
        grant: Option<&ViewGrant>,
    ) -> Result<(), AuthRejection> {
        match self.standing(endpoint, grant).await? {
            ViewStanding::Open | ViewStanding::Granted => Ok(()),
            ViewStanding::Locked => Err(AuthRejection::GrantRequired),
        }
    }

    pub async fn is_protected(&self, endpoint: &EndpointId) -> Result<bool, AuthRejection> {
        Ok(self.standing(endpoint, None).await?.is_protected())
    }

    pub async fn hash_password(
        &self,
        ip: Option<IpAddr>,
        password: &ViewPassword,
    ) -> Result<ViewPasswordHash, AuthRejection> {
        let Some(ip) = ip else {
            tracing::error!("a view password was set without a client address");
            return Err(AuthRejection::Internal);
        };
        let limits = &self.inner.limits;
        limits.tidy();
        Limits::charge(
            &limits.password_set_ip,
            &ClientKey::from_ip(ip),
            SCOPE_PASSWORD_SET_IP,
        )?;
        Ok(self.inner.passwords.hash(password).await?)
    }

    pub async fn grant_for_setter(
        &self,
        endpoint: &EndpointId,
        password_hash: &ViewPasswordHash,
    ) -> Result<Option<IssuedGrant>, AuthRejection> {
        self.issue(endpoint, password_hash).await
    }

    pub async fn unlock(
        &self,
        endpoint: &EndpointId,
        ip: Option<IpAddr>,
        password: &str,
    ) -> Result<IssuedGrant, AuthRejection> {
        let outcome = self.try_unlock(endpoint, ip, password).await;
        if matches!(outcome, Err(AuthRejection::WrongPassword)) {
            tracing::info!(endpoint_id = %endpoint, client_ip = ?ip, "an unlock attempt was refused");
        }
        outcome
    }

    async fn try_unlock(
        &self,
        endpoint: &EndpointId,
        ip: Option<IpAddr>,
        password: &str,
    ) -> Result<IssuedGrant, AuthRejection> {
        let Some(ip) = ip else {
            tracing::error!("an unlock attempt reached the service without a client address");
            return Err(AuthRejection::Internal);
        };
        let limits = &self.inner.limits;
        limits.tidy();
        let client = ClientKey::from_ip(ip);
        Limits::charge(&limits.unlock_ip, &client, SCOPE_UNLOCK_IP)?;
        let attempt = AttemptKey {
            endpoint: endpoint.clone(),
            client,
        };
        limits.ensure_not_blocked(&attempt, endpoint)?;
        if shape_of(password).is_some() {
            limits.record_failure(&attempt, endpoint);
            return Err(AuthRejection::WrongPassword);
        }
        let stored = self
            .inner
            .store
            .view_password_hash(endpoint, self.inner.retention)
            .await?;
        let Some(stored) = stored else {
            tokio::time::sleep(unprotected_delay()).await;
            limits.record_failure(&attempt, endpoint);
            return Err(AuthRejection::WrongPassword);
        };
        Limits::charge(&limits.unlock_budget, &(), SCOPE_UNLOCK_BUDGET)?;
        if !self.inner.passwords.verify(password, &stored).await? {
            limits.record_failure(&attempt, endpoint);
            return Err(AuthRejection::WrongPassword);
        }
        self.issue(endpoint, &stored)
            .await?
            .ok_or(AuthRejection::WrongPassword)
    }

    async fn issue(
        &self,
        endpoint: &EndpointId,
        password_hash: &ViewPasswordHash,
    ) -> Result<Option<IssuedGrant>, AuthRejection> {
        let grant = ViewGrant::generate().map_err(|error| {
            tracing::error!(%error, "cannot generate a view grant");
            AuthRejection::Internal
        })?;
        let issued = self
            .inner
            .store
            .issue_view_grant(
                endpoint,
                password_hash,
                grant.digest().unpack(),
                self.inner.lifetime,
                self.inner.retention,
            )
            .await?;
        Ok(issued.map(|expires_at| IssuedGrant { grant, expires_at }))
    }
}
