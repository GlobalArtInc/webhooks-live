mod dto;
mod handlers;
mod input;

use axum::Router;
use axum::routing::{get, put};

use crate::state::ApiState;

pub(crate) use handlers::vet_all;
pub(crate) use input::violations as rule_violations;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/rules",
            get(handlers::list).post(handlers::create),
        )
        .route("/endpoints/{id}/rules/order", put(handlers::reorder))
        .route(
            "/endpoints/{id}/rules/{rule_id}",
            get(handlers::show)
                .patch(handlers::update)
                .delete(handlers::delete),
        )
}
