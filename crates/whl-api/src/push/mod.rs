mod dto;
mod handlers;
mod input;

use axum::Router;
use axum::routing::{delete, get};

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/push/public-key", get(handlers::public_key))
        .route(
            "/push/subscriptions",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/push/subscriptions/{subscription_id}",
            delete(handlers::delete),
        )
}
