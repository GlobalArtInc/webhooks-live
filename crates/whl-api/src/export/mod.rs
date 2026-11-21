mod handlers;
mod params;
mod slots;
mod stream;

pub use slots::ExportSlots;

use axum::Router;
use axum::routing::get;

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new().route(
        "/endpoints/{id}/export",
        get(handlers::get).post(handlers::post).head(handlers::head),
    )
}
