mod dto;
mod handlers;
mod input;

use crate::routing::{Routes, delete, get};

pub fn routes() -> Routes {
    Routes::new()
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
