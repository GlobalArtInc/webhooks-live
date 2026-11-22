use utoipa::{OpenApi, ToSchema};

use super::dto::{
    HeaderDto, QueuedDto, RequestDto, RunDto, RunListDto, ScheduleDto, ScheduleListDto,
};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::list,
        handlers::create,
        handlers::update,
        handlers::delete,
        handlers::run,
        handlers::runs
    ),
    components(schemas(
        ScheduleInputDoc,
        ScheduleRequestInputDoc,
        ScheduleDto,
        RequestDto,
        HeaderDto,
        ScheduleListDto,
        RunDto,
        RunListDto,
        QueuedDto
    )),
    tags((name = "Schedules", description = "Cron-timed outbound requests of an endpoint; needs accounts"))
)]
pub struct SchedulesDoc;

#[derive(ToSchema)]
#[schema(as = ScheduleInput)]
#[allow(dead_code)]
pub struct ScheduleInputDoc {
    #[schema(min_length = 1, max_length = 100, example = "Nightly ping")]
    name: Option<String>,
    #[schema(max_length = 256, example = "0 3 * * *")]
    cron: Option<String>,
    #[schema(max_length = 64, example = "Europe/Berlin", default = "UTC")]
    timezone: Option<String>,
    request: Option<ScheduleRequestInputDoc>,
    #[schema(default = true)]
    enabled: Option<bool>,
}

#[derive(ToSchema)]
#[schema(as = ScheduleRequestInput)]
#[allow(dead_code)]
pub struct ScheduleRequestInputDoc {
    #[schema(example = "POST")]
    method: String,
    #[schema(example = "https://hooks.example.com/nightly")]
    url: String,
    #[schema(max_items = 20)]
    headers: Option<Vec<HeaderDto>>,
    #[schema(max_length = 65536)]
    body: Option<String>,
}
