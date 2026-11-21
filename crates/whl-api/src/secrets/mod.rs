mod dto;
mod handlers;
mod input;

use crate::routing::{Routes, get, put};

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/secrets", get(handlers::list))
        .route(
            "/endpoints/{id}/secrets/{name}",
            put(handlers::put).delete(handlers::delete),
        )
}
