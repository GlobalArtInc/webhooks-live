use axum::extract::{FromRef, FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use whl_store::UserRecord;

use crate::error::AuthRejection;
use crate::state::AuthState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUser(UserRecord);

impl CurrentUser {
    pub(crate) fn new(user: UserRecord) -> Self {
        Self(user)
    }

    pub fn unpack(&self) -> &UserRecord {
        &self.0
    }
}

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        match auth.resolve_user(&parts.headers).await? {
            Some(user) => Ok(Self(user)),
            None => Err(AuthRejection::Unauthenticated),
        }
    }
}

impl<S> OptionalFromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
    AuthState: FromRef<S>,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let auth = AuthState::from_ref(state);
        Ok(auth.resolve_user(&parts.headers).await?.map(Self))
    }
}
