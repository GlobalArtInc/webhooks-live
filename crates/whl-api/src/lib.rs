mod body;
mod captures;
mod dto;
mod error;
mod handlers;
mod input;
mod resolve;
mod settings;
mod state;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::middleware::map_response;
use axum::response::Response;
use axum::routing::{get, post};

pub use settings::ApiSettings;
pub use state::ApiState;

use error::ApiError;

pub fn router(state: ApiState) -> Router {
    let api = Router::new()
        .route("/instance", get(handlers::instance))
        .route("/endpoints", post(handlers::create))
        .route(
            "/endpoints/{id}",
            get(handlers::show)
                .patch(handlers::update)
                .delete(handlers::delete),
        )
        .merge(captures::routes())
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state);
    Router::new()
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(body::MAX_BODY_BYTES))
        .layer(map_response(secure))
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
