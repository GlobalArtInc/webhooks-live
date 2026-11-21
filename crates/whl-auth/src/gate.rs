use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{ConnectInfo, FromRequestParts, RawPathParams, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use whl_core::client_ip::TrustedProxies;
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;
use whl_core::limiter::{Limiters, Verdict, whole_seconds_up};
use whl_store::{EndpointRole, EndpointStanding, TokenScope, UserRecord, Uuid};
use whl_telemetry::series;

use crate::access::decide_access;
use crate::api_token::{self, BearerHeader, TokenGrant};
use crate::current_user::CurrentUser;
use crate::error::{AccessDenied, AuthRejection};
use crate::grant::{GrantRule, presented};
use crate::state::AuthState;
use crate::view::ViewAccess;

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
    Token {
        user: CurrentUser,
        token: TokenGrant,
        ip: Option<IpAddr>,
    },
}

impl Caller {
    pub fn user(&self) -> Option<&UserRecord> {
        match self {
            Self::Anonymous { .. } => None,
            Self::Session { user, .. } | Self::Token { user, .. } => Some(user.unpack()),
        }
    }

    pub fn token(&self) -> Option<&TokenGrant> {
        match self {
            Self::Token { token, .. } => Some(token),
            Self::Anonymous { .. } | Self::Session { .. } => None,
        }
    }

    pub fn ip(&self) -> Option<IpAddr> {
        match self {
            Self::Anonymous { ip } | Self::Session { ip, .. } | Self::Token { ip, .. } => *ip,
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

#[derive(Clone)]
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
    limiters: Option<Limiters>,
    views: Option<ViewAccess>,
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
        let views = match &mode {
            Mode::Open => None,
            Mode::Sso { auth, retention } | Mode::Mixed { auth, retention } => {
                Some(ViewAccess::new(auth.store().clone(), *retention))
            }
        };
        Self {
            inner: Arc::new(Inner {
                mode,
                proxies: TrustedProxies::default(),
                limiters: None,
                views,
            }),
        }
    }

    pub fn with_proxies(self, proxies: TrustedProxies) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: self.inner.mode.clone(),
                proxies,
                limiters: self.inner.limiters.clone(),
                views: self.inner.views.clone(),
            }),
        }
    }

    pub fn with_token_limits(self, limiters: Limiters) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: self.inner.mode.clone(),
                proxies: self.inner.proxies.clone(),
                limiters: Some(limiters),
                views: self.inner.views.clone(),
            }),
        }
    }

    pub fn with_views(self, views: ViewAccess) -> Self {
        Self {
            inner: Arc::new(Inner {
                mode: self.inner.mode.clone(),
                proxies: self.inner.proxies.clone(),
                limiters: self.inner.limiters.clone(),
                views: Some(views),
            }),
        }
    }

    pub fn views(&self) -> Option<&ViewAccess> {
        self.inner.views.as_ref()
    }

    pub fn view_access(&self) -> Result<&ViewAccess, AuthRejection> {
        self.inner.views.as_ref().ok_or_else(|| {
            tracing::error!("view passwords are not wired into this access control");
            AuthRejection::Internal
        })
    }

    pub async fn view_grant_valid(
        &self,
        endpoint: &EndpointId,
        headers: &HeaderMap,
        query: Option<&str>,
    ) -> Result<bool, AuthRejection> {
        let grant = presented(headers, query);
        match self.view_access()?.require(endpoint, grant.as_ref()).await {
            Ok(()) => Ok(true),
            Err(AuthRejection::GrantRequired) => Ok(false),
            Err(other) => Err(other),
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
        if let BearerHeader::Present(secret) = api_token::bearer(headers) {
            let (user, token) = api_token::resolve(auth.store(), secret.as_ref()).await?;
            return Ok(Caller::Token { user, token, ip });
        }
        Ok(match auth.resolve_user(headers).await? {
            Some(user) => Caller::Session {
                user: CurrentUser::new(user),
                ip,
            },
            None => Caller::Anonymous { ip },
        })
    }

    pub fn token_covers(&self, caller: &Caller, scope: TokenScope) -> bool {
        caller.token().is_none_or(|grant| grant.allows_scope(scope))
    }

    async fn standing(
        &self,
        caller: &Caller,
        endpoint: &EndpointId,
    ) -> Result<Option<EndpointStanding>, AuthRejection> {
        if let Some(grant) = caller.token()
            && !grant.allows_endpoint(endpoint)
        {
            return Ok(None);
        }
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

    pub async fn roles_on(
        &self,
        caller: &Caller,
        endpoints: &[EndpointId],
    ) -> Result<Vec<Option<EndpointRole>>, AuthRejection> {
        let user = caller.user().map(|user| user.id);
        match &self.inner.mode {
            Mode::Open => Ok(vec![None; endpoints.len()]),
            Mode::Sso { auth, retention } => match user {
                Some(user) => {
                    let roles = auth
                        .store()
                        .endpoint_roles(endpoints, user, *retention)
                        .await?;
                    Ok(endpoints
                        .iter()
                        .map(|endpoint| roles.get(endpoint.unpack()).copied())
                        .collect())
                }
                None => Ok(vec![None; endpoints.len()]),
            },
            Mode::Mixed { auth, retention } => {
                let standings = auth
                    .store()
                    .endpoint_standings(endpoints, user, *retention)
                    .await?;
                Ok(endpoints
                    .iter()
                    .map(|endpoint| {
                        standings
                            .get(endpoint.unpack())
                            .and_then(|standing| standing.role)
                    })
                    .collect())
            }
        }
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
pub enum TokenPolicy {
    Scope(TokenScope),
    SessionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutePolicy {
    pub requirement: Requirement,
    pub token: TokenPolicy,
    pub grant: GrantRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    Anyone,
    SignedIn,
    Role(EndpointRole),
    OwnedRole(EndpointRole),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointParam {
    Present(String),
    Undecodable,
    Absent,
}

impl EndpointParam {
    pub fn from_path(params: &Result<RawPathParams, RawPathParamsRejection>, name: &str) -> Self {
        match params {
            Err(_) => Self::Undecodable,
            Ok(params) => params
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map_or(Self::Absent, |(_, value)| Self::Present(value.to_owned())),
        }
    }
}

impl AccessControl {
    pub async fn enforce(
        &self,
        requirement: Requirement,
        endpoint_param: EndpointParam,
        request: Request,
        next: Next,
    ) -> Response {
        self.enforce_for(
            requirement,
            TokenPolicy::SessionOnly,
            endpoint_param,
            request,
            next,
        )
        .await
    }

    fn refuse_token(&self, caller: &Caller, policy: TokenPolicy) -> Option<AuthRejection> {
        let grant = caller.token()?;
        if let Some(limiters) = &self.inner.limiters
            && let Verdict::Limited { retry_after, .. } = limiters.check_api_token(grant.id())
        {
            metrics::counter!(series::RATE_LIMITED_TOTAL, "scope" => "api_token").increment(1);
            return Some(AuthRejection::TokenRateLimited {
                retry_after_secs: whole_seconds_up(retry_after),
            });
        }
        match policy {
            TokenPolicy::SessionOnly => Some(AuthRejection::SessionRequired),
            TokenPolicy::Scope(scope) if !grant.allows_scope(scope) => {
                Some(AuthRejection::InsufficientScope)
            }
            TokenPolicy::Scope(_) => None,
        }
    }

    pub async fn enforce_for(
        &self,
        requirement: Requirement,
        token_policy: TokenPolicy,
        endpoint_param: EndpointParam,
        request: Request,
        next: Next,
    ) -> Response {
        let route = RoutePolicy {
            requirement,
            token: token_policy,
            grant: GrantRule::Header,
        };
        self.enforce_route(route, endpoint_param, request, next)
            .await
    }

    pub async fn enforce_route(
        &self,
        route: RoutePolicy,
        endpoint_param: EndpointParam,
        mut request: Request,
        next: Next,
    ) -> Response {
        let RoutePolicy {
            requirement,
            token: token_policy,
            ..
        } = route;
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let caller = match self.caller(request.headers(), peer).await {
            Ok(caller) => caller,
            Err(rejection) => return rejection.into_response(),
        };
        if self.mode() == AccessMode::Open {
            if let Err(refusal) = self
                .require_grant(
                    &route,
                    &endpoint_param,
                    request.headers(),
                    request.uri().query(),
                    false,
                )
                .await
            {
                return refusal.into_response();
            }
            request.extensions_mut().insert(Access {
                caller: caller.clone(),
                role: None,
            });
            request.extensions_mut().insert(caller);
            return next.run(request).await;
        }
        if let Some(refusal) = self.refuse_token(&caller, token_policy) {
            return refusal.into_response();
        }
        let access = match requirement {
            Requirement::Anyone => None,
            Requirement::SignedIn => {
                if caller.user().is_none() {
                    return self.sign_in_rejection().into_response();
                }
                if let Err(refusal) = self
                    .require_grant(
                        &route,
                        &endpoint_param,
                        request.headers(),
                        request.uri().query(),
                        false,
                    )
                    .await
                {
                    return refusal.into_response();
                }
                None
            }
            Requirement::Role(needed) | Requirement::OwnedRole(needed) => {
                let owned_only = matches!(requirement, Requirement::OwnedRole(_));
                match self
                    .decide_on_endpoint(&caller, needed, owned_only, &endpoint_param)
                    .await
                {
                    Ok(decided) => {
                        if let Err(refusal) = self
                            .require_grant(
                                &route,
                                &endpoint_param,
                                request.headers(),
                                request.uri().query(),
                                decided.owned,
                            )
                            .await
                        {
                            return refusal.into_response();
                        }
                        Some(Access {
                            caller: caller.clone(),
                            role: decided.role,
                        })
                    }
                    Err(refusal) => return refusal.into_response(),
                }
            }
        };
        if endpoint_param == EndpointParam::Undecodable {
            return AuthRejection::NotFound.into_response();
        }
        request.extensions_mut().insert(caller);
        if let Some(access) = access {
            request.extensions_mut().insert(access);
        }
        next.run(request).await
    }

    async fn require_grant(
        &self,
        route: &RoutePolicy,
        endpoint_param: &EndpointParam,
        headers: &HeaderMap,
        uri_query: Option<&str>,
        owned: bool,
    ) -> Result<(), AuthRejection> {
        if owned
            || route.grant == GrantRule::NotRequired
            || route.requirement == Requirement::Anyone
        {
            return Ok(());
        }
        let EndpointParam::Present(raw) = endpoint_param else {
            return Ok(());
        };
        let Ok(endpoint) = EndpointId::parse(raw) else {
            return Ok(());
        };
        let query = if route.grant == GrantRule::HeaderOrQuery {
            uri_query
        } else {
            None
        };
        let grant = presented(headers, query);
        self.view_access()?.require(&endpoint, grant.as_ref()).await
    }

    async fn decide_on_endpoint(
        &self,
        caller: &Caller,
        needed: EndpointRole,
        owned_only: bool,
        endpoint_param: &EndpointParam,
    ) -> Result<Decided, Refusal> {
        if *endpoint_param == EndpointParam::Absent {
            tracing::error!("a role-gated route has no endpoint id parameter");
            return Err(AuthRejection::Internal.into());
        }
        let person_only = owned_only && self.is_mixed();
        if person_only && caller.user().is_none() {
            return Err(AuthRejection::SignInRequired.into());
        }
        let EndpointParam::Present(raw) = endpoint_param else {
            return Err(AuthRejection::NotFound.into());
        };
        let Ok(endpoint) = EndpointId::parse(raw) else {
            return Err(AuthRejection::NotFound.into());
        };
        if let Some(grant) = caller.token()
            && !grant.allows_endpoint(&endpoint)
        {
            return Err(AuthRejection::InsufficientScope.into());
        }
        let standing = self.standing(caller, &endpoint).await?;
        let role = standing.and_then(|standing| standing.role);
        decide_access(role, needed).into_result()?;
        if person_only && standing.is_some_and(|standing| !standing.owned) {
            return Err(AuthRejection::ClaimRequired.into());
        }
        Ok(Decided {
            role,
            owned: standing.is_some_and(|standing| standing.owned),
        })
    }
}

struct Decided {
    role: Option<EndpointRole>,
    owned: bool,
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
