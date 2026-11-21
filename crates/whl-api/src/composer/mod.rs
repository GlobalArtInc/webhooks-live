mod dto;
mod handlers;
mod input;
mod outbound;
mod redact;
mod settings;
mod target;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};

use crate::state::ApiState;

pub use settings::{
    ComposerLimits, ComposerSettings, LocalDelivery, LocalError, LocalFuture, LocalReply,
    LocalRequest, MAX_SEND_BODY_BYTES,
};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/send",
            post(handlers::send).layer(DefaultBodyLimit::max(MAX_SEND_BODY_BYTES)),
        )
        .route("/send/history", get(handlers::history))
        .route("/send/history/{sent_id}", delete(handlers::delete))
}
