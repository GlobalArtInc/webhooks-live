mod doc;
mod dto;
mod handlers;
mod input;
mod outbound;
mod redact;
mod settings;
mod target;

use crate::routing::{Routes, delete, get, post};

pub use doc::ComposerDoc;
pub use settings::{
    ComposerLimits, ComposerSettings, LocalDelivery, LocalError, LocalFuture, LocalReply,
    LocalRequest, MAX_SEND_BODY_BYTES,
};

pub fn routes() -> Routes {
    Routes::new()
        .route(
            "/send",
            post(handlers::send).body_limit(MAX_SEND_BODY_BYTES),
        )
        .route("/send/history", get(handlers::history))
        .route("/send/history/{sent_id}", delete(handlers::delete))
}
