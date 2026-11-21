mod cooldown;
mod handlers;
mod run;
mod upload;

use std::time::Duration;

use axum::Router;
use axum::routing::post;

pub use cooldown::ImportCooldown;

use crate::export::ExportSlots;
use crate::state::ApiState;

pub const MAX_IMPORT_BYTES: u64 = 100 * 1024 * 1024;
pub const IMPORT_REPLICA_CONCURRENCY: u32 = 2;
pub const UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const IMPORT_COOLDOWN: Duration = Duration::from_secs(60);
pub const MAX_IMPORT_DURATION: Duration = Duration::from_secs(10 * 60);

pub fn import_slots() -> ExportSlots {
    ExportSlots::new(IMPORT_REPLICA_CONCURRENCY)
}

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/endpoints/import", post(handlers::into_new_endpoint))
        .route("/endpoints/{id}/import", post(handlers::into_endpoint))
}
