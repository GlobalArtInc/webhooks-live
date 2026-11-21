mod annotate;
mod body;
mod cursor;
mod dto;
mod handlers;
mod query;
mod stats;

use axum::Router;
use axum::routing::get;

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/captures",
            get(handlers::list).delete(handlers::clear),
        )
        .route("/endpoints/{id}/captures/latest", get(handlers::latest))
        .route("/endpoints/{id}/stats", get(stats::show))
        .route(
            "/endpoints/{id}/captures/{capture_id}/body",
            get(body::download),
        )
        .route(
            "/endpoints/{id}/captures/{capture_id}",
            get(handlers::show)
                .patch(annotate::update)
                .delete(handlers::delete),
        )
}
