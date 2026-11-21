mod dto;
mod handlers;
mod input;

use axum::Router;
use axum::routing::{get, put};

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/endpoints/{id}/secrets", get(handlers::list))
        .route(
            "/endpoints/{id}/secrets/{name}",
            put(handlers::put).delete(handlers::delete),
        )
}
