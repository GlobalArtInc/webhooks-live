mod body;
mod cursor;
mod dto;
mod handlers;
mod query;

use axum::Router;
use axum::routing::get;

pub(crate) use dto::SummaryDto;
pub(crate) use query::{ListParams, inbox_query};

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/captures",
            get(handlers::list).delete(handlers::clear),
        )
        .route("/endpoints/{id}/captures/latest", get(handlers::latest))
        .route(
            "/endpoints/{id}/captures/{capture_id}/body",
            get(body::download),
        )
        .route(
            "/endpoints/{id}/captures/{capture_id}",
            get(handlers::show).delete(handlers::delete),
        )
}
