mod doc;
mod document;
mod export;
mod handlers;
mod prepare;
mod request;
mod scan;
mod templates;

use crate::routing::{Routes, get, post};

pub use doc::CloningDoc;
pub(crate) use templates::{create_from_template, split_template_request};

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/clone", post(handlers::clone_endpoint))
        .route("/templates", get(templates::list).post(templates::create))
        .route(
            "/templates/{template_id}",
            get(templates::show)
                .patch(templates::rename)
                .delete(templates::destroy),
        )
}
