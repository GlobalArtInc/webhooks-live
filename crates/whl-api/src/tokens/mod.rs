mod doc;
mod dto;
mod handlers;
mod input;

use crate::routing::{Routes, delete, get};

pub use doc::TokensDoc;

pub const MAX_TOKENS_PER_HOUR: u32 = 10;

pub fn routes() -> Routes {
    Routes::new()
        .route("/tokens", get(handlers::list).post(handlers::create))
        .route("/tokens/{token_id}", delete(handlers::revoke))
}
