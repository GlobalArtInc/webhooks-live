mod common;
pub(crate) mod params;
mod registry;
pub(crate) mod responses;
mod root;
mod serve;

#[cfg(test)]
mod coverage;
#[cfg(test)]
mod structure;
#[cfg(test)]
mod undocumented;

use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

use crate::routing::{Routes, get};

pub fn routes() -> Routes {
    Routes::new().route("/openapi.json", get(serve::show))
}

pub(crate) fn document() -> Document {
    let mut document = root::RootDoc::openapi();
    for area in registry::area_docs() {
        document.merge(area);
    }
    common::add_common_responses(&mut document);
    document
}
