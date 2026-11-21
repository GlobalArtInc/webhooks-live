mod access;
mod api_token;
mod cookies;
mod current_user;
mod error;
mod failures;
mod flow;
mod gate;
mod grant;
mod origin;
mod password;
mod provider;
mod return_to;
mod settings;
mod state;
mod token;
mod view;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::map_response;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};

pub use access::{Decision, decide_access};
pub use api_token::{
    ApiTokenHash, ApiTokenSecret, LAST_USED_INTERVAL, LOOKUP_PREFIX_CHARS, TOKEN_PREFIX, TokenGrant,
};
pub use cookies::{CookiePolicy, LoginCookie};
pub use current_user::CurrentUser;
pub use error::{AccessDenied, AuthError, AuthRejection, LoginFailure};
pub use failures::FailureSpec;
pub use gate::{
    Access, AccessControl, AccessMode, Caller, Creation, EndpointParam, Requirement, RoutePolicy,
    TokenPolicy,
};
pub use grant::{GRANT_HEADER, GRANT_LIFETIME, GRANT_QUERY_KEY, GrantRule, ViewGrant};
pub use origin::{OriginCheck, OriginCheckService};
pub use password::{
    HASH_WAIT, MAX_CONCURRENT_HASHES, MAX_CONCURRENT_VERIFIES, MAX_PASSWORD_CHARS,
    MIN_PASSWORD_CHARS, PasswordError, PasswordShape, ViewPassword, ViewPasswords,
};
pub use provider::CLOCK_LEEWAY;
pub use return_to::sanitize as sanitize_return_to;
pub use settings::{
    AuthSettings, DEFAULT_MAX_PENDING_LOGINS, DEFAULT_MAX_PENDING_LOGINS_PER_NETWORK, OidcClient,
    SessionLimits,
};
pub use state::{AuthState, DiscoveryBackoff};
pub use view::{IssuedGrant, ViewAccess, ViewLimitSettings};
pub use whl_store::{EndpointRole, TokenScope};

pub fn router(state: AuthState) -> Router {
    let origin = state.origin_check();
    let routes = Router::new()
        .route("/login", get(flow::login))
        .route("/callback", get(flow::callback))
        .route("/logout", post(flow::logout))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state);
    Router::new()
        .nest("/auth", routes)
        .layer(map_response(harden))
        .layer(origin)
}

pub fn unavailable_router() -> Router {
    Router::new()
        .route("/auth", any(not_available))
        .route("/auth/{*rest}", any(not_available))
}

async fn not_found() -> AuthRejection {
    AuthRejection::NotFound
}

async fn method_not_allowed() -> AuthRejection {
    AuthRejection::MethodNotAllowed
}

async fn not_available(_request: Request) -> Response {
    AuthRejection::NotAvailableInMode.into_response()
}

async fn harden(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}
