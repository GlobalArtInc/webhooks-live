mod handlers;
mod params;
mod slots;
mod stream;

pub use slots::ExportSlots;

use crate::routing::{Routes, get};

pub fn routes() -> Routes {
    Routes::new().route(
        "/endpoints/{id}/export",
        get(handlers::get).post(handlers::post).head(handlers::head),
    )
}
