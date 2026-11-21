mod dry_run;
mod dto;
mod handlers;
mod input;
mod runs;

pub(crate) use runs::RuleRunDto;

use axum::Router;
use axum::routing::{get, post, put};

use crate::state::ApiState;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/rules",
            get(handlers::list).post(handlers::create),
        )
        .route("/endpoints/{id}/rules/order", put(handlers::reorder))
        .route("/endpoints/{id}/rules/dry-run", post(dry_run::dry_run))
        .route("/endpoints/{id}/rule-runs", get(runs::list))
        .route(
            "/endpoints/{id}/captures/{capture_id}/rule-runs",
            get(runs::for_capture).post(runs::rerun),
        )
        .route(
            "/endpoints/{id}/rules/{rule_id}",
            get(handlers::show)
                .patch(handlers::update)
                .delete(handlers::delete),
        )
}
