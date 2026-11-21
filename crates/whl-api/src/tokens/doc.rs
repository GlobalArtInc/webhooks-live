use utoipa::OpenApi;

use super::dto::{ApiTokenDto, ApiTokenInputDoc, ApiTokenListDto, CreatedApiTokenDto};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::list, handlers::create, handlers::revoke),
    components(schemas(ApiTokenDto, ApiTokenInputDoc, ApiTokenListDto, CreatedApiTokenDto)),
    tags((name = "Tokens", description = "Personal API tokens; session-only routes"))
)]
pub struct TokensDoc;
