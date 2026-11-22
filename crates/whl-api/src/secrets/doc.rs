use utoipa::OpenApi;

use super::dto::{SecretDto, SecretInputDoc, SecretListDto};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::list, handlers::put, handlers::delete),
    components(schemas(SecretDto, SecretInputDoc, SecretListDto)),
    tags((name = "Secrets", description = "Encrypted secret values an endpoint's rules and integrations reference by name; values are write-only"))
)]
pub struct SecretsDoc;
