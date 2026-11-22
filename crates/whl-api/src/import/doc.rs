use utoipa::OpenApi;

use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::into_new_endpoint, handlers::into_endpoint),
    components(schemas(handlers::ImportDto)),
    tags((name = "Import", description = "Load the captures of an export file (JSON or NDJSON) into an endpoint or a new one; session only"))
)]
pub struct ImportDoc;
