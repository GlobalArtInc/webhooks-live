mod document;
mod export;
mod handlers;
mod prepare;
mod request;
mod scan;
mod templates;

use axum::Router;
use axum::routing::{get, post};

use crate::state::ApiState;

pub(crate) use templates::{create_from_template, split_template_request};

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route("/endpoints/{id}/clone", post(handlers::clone_endpoint))
        .route("/templates", get(templates::list).post(templates::create))
        .route(
            "/templates/{template_id}",
            get(templates::show)
                .patch(templates::rename)
                .delete(templates::destroy),
        )
}
