use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{FromRequestParts, Request};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use whl_core::endpoint_id::EndpointId;
use whl_store::{EndpointRole, UserRecord};

use crate::access::decide_access;
use crate::current_user::CurrentUser;
use crate::error::AuthRejection;
use crate::state::AuthState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Anonymous,
    Session(CurrentUser),
}

impl Caller {
    pub fn user(&self) -> Option<&UserRecord> {
        match self {
            Self::Anonymous => None,
            Self::Session(user) => Some(user.unpack()),
        }
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

enum Mode {
    Open,
    Sso {
        auth: AuthState,
        retention: Duration,
    },
}

#[derive(Clone)]
pub struct AccessControl {
    mode: Arc<Mode>,
}

impl fmt::Debug for AccessControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode.as_ref() {
            Mode::Open => "open",
            Mode::Sso { .. } => "sso",
        };
        f.debug_struct("AccessControl")
            .field("mode", &mode)
            .finish()
    }
}

impl AccessControl {
    pub fn open() -> Self {
        Self {
            mode: Arc::new(Mode::Open),
        }
    }

    pub fn sso(auth: AuthState, retention: Duration) -> Self {
        Self {
            mode: Arc::new(Mode::Sso { auth, retention }),
        }
    }

    pub fn is_sso(&self) -> bool {
        matches!(self.mode.as_ref(), Mode::Sso { .. })
    }

    pub fn provider_name(&self) -> Option<&str> {
        match self.mode.as_ref() {
            Mode::Open => None,
            Mode::Sso { auth, .. } => auth.settings().provider_name(),
        }
    }

    pub async fn caller(&self, headers: &HeaderMap) -> Result<Caller, AuthRejection> {
        match self.mode.as_ref() {
            Mode::Open => Ok(Caller::Anonymous),
            Mode::Sso { auth, .. } => Ok(match auth.resolve_user(headers).await? {
                Some(user) => Caller::Session(CurrentUser::new(user)),
                None => Caller::Anonymous,
            }),
        }
    }

    pub async fn role_on(
        &self,
        caller: &Caller,
        endpoint: &EndpointId,
    ) -> Result<Option<EndpointRole>, AuthRejection> {
        match (self.mode.as_ref(), caller.user()) {
            (Mode::Sso { auth, retention }, Some(user)) => Ok(auth
                .store()
                .endpoint_role(endpoint, user.id, *retention)
                .await?),
            (Mode::Sso { .. }, None) | (Mode::Open, _) => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    Anyone,
    SignedIn,
    Role(EndpointRole),
}

impl AccessControl {
    pub async fn enforce(
        &self,
        requirement: Requirement,
        endpoint_param: Option<&str>,
        mut request: Request,
        next: Next,
    ) -> Response {
        if !self.is_sso() {
            request.extensions_mut().insert(Caller::Anonymous);
            request.extensions_mut().insert(Access {
                caller: Caller::Anonymous,
                role: None,
            });
            return next.run(request).await;
        }
        let caller = match self.caller(request.headers()).await {
            Ok(caller) => caller,
            Err(rejection) => return rejection.into_response(),
        };
        let access = match requirement {
            Requirement::Anyone => None,
            Requirement::SignedIn => {
                if caller.user().is_none() {
                    return AuthRejection::Unauthenticated.into_response();
                }
                None
            }
            Requirement::Role(needed) => {
                let Some(raw) = endpoint_param else {
                    tracing::error!("a role-gated route has no endpoint id parameter");
                    return AuthRejection::Internal.into_response();
                };
                let Ok(endpoint) = EndpointId::parse(raw) else {
                    return AuthRejection::NotFound.into_response();
                };
                let role = match self.role_on(&caller, &endpoint).await {
                    Ok(role) => role,
                    Err(rejection) => return rejection.into_response(),
                };
                if let Err(denied) = decide_access(role, needed).into_result() {
                    return denied.into_response();
                }
                Some(Access {
                    caller: caller.clone(),
                    role,
                })
            }
        };
        request.extensions_mut().insert(caller);
        if let Some(access) = access {
            request.extensions_mut().insert(access);
        }
        next.run(request).await
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
