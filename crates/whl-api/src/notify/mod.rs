mod dto;
mod email;
mod handlers;
mod input;
mod settings;

use axum::Router;
use axum::routing::{get, patch, post};

use crate::state::ApiState;

pub(crate) use dto::DeliveryDto;
pub use email::{EmailApi, EmailApiLimits};
pub(crate) use handlers::build_channel;
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
        .route("/notifications/disable", post(email::disable))
        .route("/email-addresses", get(email::list).post(email::add))
        .route("/email-addresses/verify", post(email::verify))
        .route(
            "/email-addresses/{address_id}",
            axum::routing::delete(email::remove),
        )
}
