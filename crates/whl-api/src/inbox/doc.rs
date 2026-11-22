use utoipa::OpenApi;

use super::list::{self, InboxListDto};

#[derive(OpenApi)]
#[openapi(
    paths(list::list),
    components(schemas(InboxListDto)),
    tags((name = "Inbox", description = "The newest captures of several endpoints in one list; session only"))
)]
pub struct InboxDoc;
