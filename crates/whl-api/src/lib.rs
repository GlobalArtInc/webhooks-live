mod body;
mod captures;
mod claim;
mod cors;
mod dto;
mod error;
mod folders;
mod forward;
mod handlers;
mod input;
mod lifecycle;
mod limit;
mod listing;
mod members;
mod policy;
mod resolve;
mod rules;
mod secrets;
mod settings;
mod state;
mod stream;
mod tokens;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::middleware::{from_fn_with_state, map_response};
use axum::response::Response;
use axum::routing::{get, post};

pub use cors::api_cors;
pub use forward::ForwardSettings;
pub use limit::{ApiLimits, enforce_rate_limit};
pub use policy::guard_routes;
pub use settings::ApiSettings;
pub use state::ApiState;
pub use stream::stream_authorizer;

use error::ApiError;

pub fn router(state: ApiState) -> Router {
    let limits = state.limits.clone();
    let api = Router::new()
        .route("/instance", get(handlers::instance))
        .route("/endpoints", get(listing::list).post(handlers::create))
        .route(
            "/endpoints/{id}",
            get(handlers::show)
                .patch(handlers::update)
                .delete(handlers::delete),
        )
        .route("/endpoints/{id}/claim", post(claim::claim))
        .merge(members::routes())
        .merge(captures::routes())
        .merge(folders::routes())
        .merge(forward::routes())
        .merge(rules::routes())
        .merge(secrets::routes())
        .merge(tokens::routes());
    let api = guard_routes(api, &state.access)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state);
    let router = Router::new()
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(body::MAX_BODY_BYTES));
    let router = match limits {
        Some(limits) => router.layer(from_fn_with_state(limits, enforce_rate_limit)),
        None => router,
    };
    router.layer(map_response(secure))
}

async fn not_found() -> ApiError {
    ApiError::NotFound
}

async fn method_not_allowed() -> ApiError {
    ApiError::MethodNotAllowed
}

async fn secure(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
