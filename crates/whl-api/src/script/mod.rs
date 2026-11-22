mod doc;
mod dto;
mod handlers;
mod input;

use crate::routing::{Routes, get, post};

pub use doc::ScriptDoc;
pub(crate) use handlers::SCRIPT_CHECK_SLOTS;
pub(crate) use input::MAX_VALIDATE_BODY_BYTES;

pub fn routes() -> Routes {
    Routes::new()
        .route("/script/meta", get(handlers::meta))
        .route(
            "/script/validate",
            post(handlers::validate).body_limit(MAX_VALIDATE_BODY_BYTES),
        )
}

#[cfg(test)]
mod tests;
