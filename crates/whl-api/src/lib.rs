mod access;
mod aliases;
mod body;
mod captures;
mod claim;
mod cloning;
mod composer;
mod cors;
mod dto;
mod error;
mod export;
mod folders;
mod forward;
mod handlers;
mod import;
mod inbox;
mod input;
mod lifecycle;
mod limit;
mod listing;
mod members;
mod notify;
mod openapi;
mod policy;
mod push;
mod quota;
mod resolve;
mod routing;
mod rules;
mod schedules;
mod script;
mod secrets;
mod settings;
mod state;
mod stream;
mod templating;
mod tokens;
mod vars;
mod view_password;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, header};
use axum::middleware::{from_fn_with_state, map_response};
use axum::response::Response;

pub use composer::{
    ComposerLimits, ComposerSettings, LocalDelivery, LocalError, LocalFuture, LocalReply,
    LocalRequest, MAX_SEND_BODY_BYTES,
};
pub use cors::api_cors;
pub use forward::ForwardSettings;
pub use inbox::inbox_selector;
pub use limit::{ApiLimits, enforce_rate_limit};
pub use notify::{EmailApi, EmailApiLimits, NotifyApiSettings};
pub use policy::guard_routes;
pub use schedules::ScheduleSettings;
pub use settings::ApiSettings;
pub use state::ApiState;
pub use stream::stream_authorizer;

use error::ApiError;
use routing::{Routes, get, post};

fn api_routes() -> Routes {
    Routes::new()
        .route("/instance", get(handlers::instance))
        .route("/endpoints", get(listing::list).post(handlers::create))
        .route(
            "/endpoints/{id}",
            get(handlers::show)
                .patch(handlers::update)
                .delete(handlers::delete),
        )
        .route("/endpoints/{id}/claim", post(claim::claim))
        .route("/endpoints/{id}/unlock", post(view_password::unlock))
        .merge(aliases::routes())
        .merge(members::routes())
        .merge(captures::routes())
        .merge(folders::routes())
        .merge(cloning::routes())
        .merge(export::routes())
        .merge(inbox::routes())
        .merge(import::routes())
        .merge(forward::routes())
        .merge(rules::routes())
        .merge(script::routes())
        .merge(schedules::routes())
        .merge(notify::routes())
        .merge(push::routes())
        .merge(secrets::routes())
        .merge(composer::routes())
        .merge(tokens::routes())
        .merge(quota::routes())
        .merge(templating::routes())
        .merge(vars::routes())
        .merge(openapi::routes())
}

pub fn router(state: ApiState) -> Router {
    let limits = state.limits.clone();
    let api = guard_routes(api_routes().into_router(), &state.access)
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
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::*;

    async fn cache_control(path: &str) -> (HeaderValue, HeaderValue) {
        let router = Router::new()
            .route("/plain", get(|| async { "ok" }))
            .route(
                "/own",
                get(|| async { ([(header::CACHE_CONTROL, "no-cache")], "ok") }),
            )
            .layer(map_response(secure));
        let request = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("a valid request");
        let reply = router.oneshot(request).await.expect("a reply");
        (
            reply.headers()[header::CACHE_CONTROL].clone(),
            reply.headers()[header::X_CONTENT_TYPE_OPTIONS].clone(),
        )
    }

    #[tokio::test]
    async fn a_response_without_cache_control_is_stored_nowhere() {
        let (cache, sniff) = cache_control("/plain").await;
        assert_eq!(cache, "no-store");
        assert_eq!(sniff, "nosniff");
    }

    #[tokio::test]
    async fn a_response_that_sets_its_own_cache_control_keeps_it() {
        let (cache, sniff) = cache_control("/own").await;
        assert_eq!(cache, "no-cache");
        assert_eq!(sniff, "nosniff");
    }
}
