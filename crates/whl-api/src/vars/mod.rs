mod dto;
mod handlers;
mod input;

use axum::Router;
use axum::routing::{get, put};

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/endpoints/{id}/vars", get(handlers::list))
        .route(
            "/endpoints/{id}/vars/{name}",
            put(handlers::put).delete(handlers::delete),
        )
}
