mod dto;
mod handlers;
mod input;

use crate::routing::{Routes, get, put};

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/vars", get(handlers::list))
        .route(
            "/endpoints/{id}/vars/{name}",
            put(handlers::put).delete(handlers::delete),
        )
}
