mod dto;
mod handlers;
mod input;

use axum::Router;
use axum::routing::{delete, get};

use crate::state::ApiState;

pub const MAX_TOKENS_PER_USER: u32 = 20;
pub const MAX_TOKENS_PER_HOUR: u32 = 10;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/tokens", get(handlers::list).post(handlers::create))
        .route("/tokens/{token_id}", delete(handlers::revoke))
}
