mod doc;
mod dto;
mod input;
mod preview;
mod slots;
mod subject;
mod variables;

use crate::routing::Routes;
use crate::routing::{get, post};

pub use doc::TemplatingDoc;
pub(crate) use slots::PreviewSlots;

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/template/preview", post(preview::preview))
        .route(
            "/endpoints/{id}/template/variables",
            get(variables::variables),
        )
}
