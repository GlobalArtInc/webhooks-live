mod doc;
mod dto;
mod handlers;
mod input;
mod settings;

use crate::routing::{Routes, get, patch, post};

pub use doc::ForwardDoc;
pub(crate) use dto::{JobDto, RuleDto};
pub(crate) use handlers::build_rule;
pub(crate) use input::target_violation_for;
pub use settings::ForwardSettings;

pub fn routes() -> Routes {
    Routes::new()
        .route(
            "/endpoints/{id}/forward-rules",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/endpoints/{id}/forward-rules/{rule_id}",
            patch(handlers::update).delete(handlers::delete),
        )
        .route(
            "/endpoints/{id}/captures/{capture_id}/forward",
            post(handlers::resend),
        )
        .route(
            "/endpoints/{id}/captures/{capture_id}/resend",
            post(handlers::resend),
        )
}
