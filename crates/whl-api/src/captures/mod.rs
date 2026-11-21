mod annotate;
mod body;
mod cursor;
mod dto;
mod handlers;
mod parts;
pub(crate) mod query;
mod stats;

pub(crate) use dto::SummaryDto;
pub(crate) use query::{ListParams, inbox_query};

use crate::routing::{Routes, get};

pub fn routes() -> Routes {
    Routes::new()
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
            "/endpoints/{id}/captures/{capture_id}/parts/{index}",
            get(parts::download),
        )
        .route(
            "/endpoints/{id}/captures/{capture_id}",
            get(handlers::show)
                .patch(annotate::update)
                .delete(handlers::delete),
        )
}
