mod dto;
mod handlers;
mod input;
mod settings;

use axum::Router;
use axum::routing::{get, patch, post};

use crate::state::ApiState;

pub(crate) use dto::DeliveryDto;
pub(crate) use input::object_of;
pub use settings::NotifyApiSettings;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/notifications",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/endpoints/{id}/notifications/{channel_id}",
            patch(handlers::update).delete(handlers::delete),
        )
        .route(
            "/endpoints/{id}/notifications/{channel_id}/test",
            post(handlers::test),
        )
}
