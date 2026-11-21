use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, FromRequestParts, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use whl_core::client_ip::TrustedProxies;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointRole, EndpointStanding, UserRecord, Uuid};

use crate::access::decide_access;
use crate::current_user::CurrentUser;
use crate::error::{AccessDenied, AuthRejection};
use crate::state::AuthState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Open,
    Sso,
    Mixed,
}

impl AccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Sso => "sso",
            Self::Mixed => "mixed",
        }
    }

    pub fn has_users(self) -> bool {
        matches!(self, Self::Sso | Self::Mixed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Anonymous {
        ip: Option<IpAddr>,
    },
    Session {
        user: CurrentUser,
        ip: Option<IpAddr>,
    },
}

impl Caller {
    pub fn user(&self) -> Option<&UserRecord> {
        match self {
            Self::Anonymous { .. } => None,
            Self::Session { user, .. } => Some(user.unpack()),
        }
    }

    pub fn ip(&self) -> Option<IpAddr> {
        match self {
            Self::Anonymous { ip } | Self::Session { ip, .. } => *ip,
        }
    }

    pub fn network(&self) -> Option<CreatorNet> {
        self.ip().map(CreatorNet::from_ip)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    caller: Caller,
    role: Option<EndpointRole>,
}

impl Access {
    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn role(&self) -> Option<EndpointRole> {
        self.role
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Creation {
    Owned(Uuid),
    Anonymous { network: Option<CreatorNet> },
}

enum Mode {
    Open,
    Sso {
        auth: AuthState,
        retention: Duration,
    },
    Mixed {
        auth: AuthState,
        retention: Duration,
    },
}

struct Inner {
    mode: Mode,
    proxies: TrustedProxies,
}

#[derive(Clone)]
pub struct AccessControl {
    inner: Arc<Inner>,
}

impl fmt::Debug for AccessControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessControl")
            .field("mode", &self.mode().as_str())
            .finish()
    }
}

impl AccessControl {
    pub fn open() -> Self {
        Self::build(Mode::Open)
    }

    pub fn sso(auth: AuthState, retention: Duration) -> Self {
        Self::build(Mode::Sso { auth, retention })
    }

    pub fn mixed(auth: AuthState, retention: Duration) -> Self {
        Self::build(Mode::Mixed { auth, retention })
    }

    fn build(mode: Mode) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode,
                proxies: TrustedProxies::default(),
            }),
        }
    }

    pub fn with_proxies(self, proxies: TrustedProxies) -> Self {
        let mode = match &self.inner.mode {
            Mode::Open => Mode::Open,
            Mode::Sso { auth, retention } => Mode::Sso {
                auth: auth.clone(),
                retention: *retention,
            },
            Mode::Mixed { auth, retention } => Mode::Mixed {
                auth: auth.clone(),
                retention: *retention,
            },
        };
        Self {
            inner: Arc::new(Inner { mode, proxies }),
        }
    }

    pub fn mode(&self) -> AccessMode {
        match self.inner.mode {
            Mode::Open => AccessMode::Open,
            Mode::Sso { .. } => AccessMode::Sso,
            Mode::Mixed { .. } => AccessMode::Mixed,
        }
    }

    pub fn is_sso(&self) -> bool {
        self.mode() == AccessMode::Sso
    }

    pub fn is_mixed(&self) -> bool {
        self.mode() == AccessMode::Mixed
    }

    pub fn has_users(&self) -> bool {
        self.mode().has_users()
    }

    fn auth(&self) -> Option<&AuthState> {
        match &self.inner.mode {
            Mode::Open => None,
            Mode::Sso { auth, .. } | Mode::Mixed { auth, .. } => Some(auth),
        }
    }

    pub fn provider_name(&self) -> Option<&str> {
        self.auth().and_then(|auth| auth.settings().provider_name())
    }

    pub fn sign_in_available(&self) -> bool {
        self.auth().is_some_and(AuthState::is_available)
    }

    pub async fn caller(
        &self,
        headers: &HeaderMap,
        peer: Option<IpAddr>,
    ) -> Result<Caller, AuthRejection> {
        let ip = peer.map(|peer| self.inner.proxies.resolve(peer, headers).client_ip);
        let Some(auth) = self.auth() else {
            return Ok(Caller::Anonymous { ip });
        };
        Ok(match auth.resolve_user(headers).await? {
            Some(user) => Caller::Session {
                user: CurrentUser::new(user),
                ip,
            },
            None => Caller::Anonymous { ip },
        })
    }

    async fn standing(
        &self,
        caller: &Caller,
        endpoint: &EndpointId,
    ) -> Result<Option<EndpointStanding>, AuthRejection> {
        let user = caller.user().map(|user| user.id);
        match &self.inner.mode {
            Mode::Open => Ok(None),
            Mode::Sso { auth, retention } => Ok(match user {
                Some(user) => {
                    let role = auth
                        .store()
                        .endpoint_role(endpoint, user, *retention)
                        .await?;
                    role.map(|role| EndpointStanding {
                        owned: true,
                        role: Some(role),
                    })
                }
                None => None,
            }),
            Mode::Mixed { auth, retention } => Ok(auth
                .store()
                .endpoint_standing(endpoint, user, *retention)
                .await?),
        }
    }

    pub async fn role_on(
        &self,
        caller: &Caller,
        endpoint: &EndpointId,
    ) -> Result<Option<EndpointRole>, AuthRejection> {
        Ok(self
            .standing(caller, endpoint)
            .await?
            .and_then(|standing| standing.role))
    }

    pub fn creation(&self, caller: &Caller) -> Result<Creation, AuthRejection> {
        match (self.mode(), caller.user()) {
            (AccessMode::Open, _) => Ok(Creation::Anonymous { network: None }),
            (AccessMode::Sso | AccessMode::Mixed, Some(user)) => Ok(Creation::Owned(user.id)),
            (AccessMode::Mixed, None) => Ok(Creation::Anonymous {
                network: caller.network(),
            }),
            (AccessMode::Sso, None) => Err(AuthRejection::Unauthenticated),
        }
    }

    fn sign_in_rejection(&self) -> AuthRejection {
        match self.mode() {
            AccessMode::Mixed => AuthRejection::SignInRequired,
            AccessMode::Open | AccessMode::Sso => AuthRejection::Unauthenticated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    Anyone,
    SignedIn,
    Role(EndpointRole),
    OwnedRole(EndpointRole),
}

impl AccessControl {
    pub async fn enforce(
        &self,
        requirement: Requirement,
        endpoint_param: Option<&str>,
        mut request: Request,
        next: Next,
    ) -> Response {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let caller = match self.caller(request.headers(), peer).await {
            Ok(caller) => caller,
            Err(rejection) => return rejection.into_response(),
        };
        if self.mode() == AccessMode::Open {
            request.extensions_mut().insert(Access {
                caller: caller.clone(),
                role: None,
            });
            request.extensions_mut().insert(caller);
            return next.run(request).await;
        }
        let access = match requirement {
            Requirement::Anyone => None,
            Requirement::SignedIn => {
                if caller.user().is_none() {
                    return self.sign_in_rejection().into_response();
                }
                None
            }
            Requirement::Role(needed) | Requirement::OwnedRole(needed) => {
                let owned_only = matches!(requirement, Requirement::OwnedRole(_));
                match self
                    .decide_on_endpoint(&caller, needed, owned_only, endpoint_param)
                    .await
                {
                    Ok(role) => Some(Access {
                        caller: caller.clone(),
                        role,
                    }),
                    Err(refusal) => return refusal.into_response(),
                }
            }
        };
        request.extensions_mut().insert(caller);
        if let Some(access) = access {
            request.extensions_mut().insert(access);
        }
        next.run(request).await
    }

    async fn decide_on_endpoint(
        &self,
        caller: &Caller,
        needed: EndpointRole,
        owned_only: bool,
        endpoint_param: Option<&str>,
    ) -> Result<Option<EndpointRole>, Refusal> {
        let Some(raw) = endpoint_param else {
            tracing::error!("a role-gated route has no endpoint id parameter");
            return Err(AuthRejection::Internal.into());
        };
        let person_only = owned_only && self.is_mixed();
        if person_only && caller.user().is_none() {
            return Err(AuthRejection::SignInRequired.into());
        }
        let Ok(endpoint) = EndpointId::parse(raw) else {
            return Err(AuthRejection::NotFound.into());
        };
        let standing = self.standing(caller, &endpoint).await?;
        let role = standing.and_then(|standing| standing.role);
        decide_access(role, needed).into_result()?;
        if person_only && standing.is_some_and(|standing| !standing.owned) {
            return Err(AuthRejection::ClaimRequired.into());
        }
        Ok(role)
    }
}

enum Refusal {
    Rejected(AuthRejection),
    Denied(AccessDenied),
}

impl From<AuthRejection> for Refusal {
    fn from(rejection: AuthRejection) -> Self {
        Self::Rejected(rejection)
    }
}

impl From<AccessDenied> for Refusal {
    fn from(denied: AccessDenied) -> Self {
        Self::Denied(denied)
    }
}

impl IntoResponse for Refusal {
    fn into_response(self) -> Response {
        match self {
            Self::Rejected(rejection) => rejection.into_response(),
            Self::Denied(denied) => denied.into_response(),
        }
    }
}

fn missing(kind: &'static str) -> AuthRejection {
    tracing::error!(
        kind,
        "a handler ran without the access layer in front of it"
    );
    AuthRejection::Internal
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or_else(|| missing("caller"))
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Access {
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Self>()
            .cloned()
            .ok_or_else(|| missing("access"))
    }
}
