mod cooldown;
mod doc;
mod handlers;
mod run;
mod upload;

use std::time::Duration;

pub use cooldown::ImportCooldown;
pub use doc::ImportDoc;

use crate::export::ExportSlots;
use crate::routing::{Routes, post};

pub const IMPORT_REPLICA_CONCURRENCY: u32 = 2;
pub const UPLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
pub const IMPORT_COOLDOWN: Duration = Duration::from_secs(60);
pub const MAX_IMPORT_DURATION: Duration = Duration::from_secs(10 * 60);

pub fn import_slots() -> ExportSlots {
    ExportSlots::new(IMPORT_REPLICA_CONCURRENCY)
}

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/import", post(handlers::into_new_endpoint))
        .route("/endpoints/{id}/import", post(handlers::into_endpoint))
}
