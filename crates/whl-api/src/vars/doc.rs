use utoipa::OpenApi;

use super::dto::{VarDto, VarInputDoc, VarListDto};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::list, handlers::put, handlers::delete),
    components(schemas(VarDto, VarInputDoc, VarListDto)),
    tags((name = "Variables", description = "Persistent variables of an endpoint: named JSON values that rules read, set and count"))
)]
pub struct VarsDoc;
