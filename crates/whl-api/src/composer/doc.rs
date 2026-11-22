use utoipa::OpenApi;

use super::dto::{HistoryDto, SendRequestDoc, SendResultDto};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::send, handlers::history, handlers::delete),
    components(schemas(HistoryDto, SendRequestDoc, SendResultDto)),
    tags((name = "Composer", description = "Send an HTTP request from the instance and keep the history of the sent requests"))
)]
pub struct ComposerDoc;
