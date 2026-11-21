mod dto;
mod handlers;
mod input;
mod settings;

use axum::Router;
use axum::routing::{get, patch, post};

use crate::state::ApiState;

pub(crate) use handlers::{check_cron, check_request_with};
pub(crate) use input::{ParsedSchedule, parse_schedule};
pub use settings::ScheduleSettings;

pub(crate) const MAX_HEADERS_INPUT: usize = whl_schedule::MAX_HEADERS;

pub fn routes() -> Router<ApiState> {
    Router::new()
        .route(
            "/endpoints/{id}/schedules",
            get(handlers::list).post(handlers::create),
        )
        .route(
            "/endpoints/{id}/schedules/{schedule_id}",
            patch(handlers::update).delete(handlers::delete),
        )
        .route(
            "/endpoints/{id}/schedules/{schedule_id}/run",
            post(handlers::run),
        )
        .route(
            "/endpoints/{id}/schedules/{schedule_id}/runs",
            get(handlers::runs),
        )
}
