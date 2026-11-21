use serde::Serialize;
use utoipa::ToSchema;
use whl_store::ApiTokenRecord;

use crate::dto::timestamp;

#[derive(Serialize, ToSchema)]
#[schema(as = ApiToken)]
pub struct ApiTokenDto {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(example = "laptop cli")]
    name: String,
    #[schema(example = "whl_q3k7x2m9")]
    prefix: String,
    #[schema(example = json!(["read"]))]
    scopes: Vec<&'static str>,
    endpoint_ids: Option<Vec<String>>,
    #[schema(format = DateTime)]
    created_at: String,
    #[schema(format = DateTime)]
    expires_at: String,
    #[schema(format = DateTime)]
    last_used_at: Option<String>,
    #[schema(format = DateTime)]
    revoked_at: Option<String>,
}

impl From<&ApiTokenRecord> for ApiTokenDto {
    fn from(record: &ApiTokenRecord) -> Self {
        Self {
            id: record.id.hyphenated().to_string(),
            name: record.name.clone(),
            prefix: record.prefix.clone(),
            scopes: record.scopes.iter().map(|scope| scope.as_str()).collect(),
            endpoint_ids: record.endpoint_ids.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| id.unpack().to_owned())
                    .collect::<Vec<_>>()
            }),
            created_at: timestamp(record.created_at),
            expires_at: timestamp(record.expires_at),
            last_used_at: record.last_used_at.map(timestamp),
            revoked_at: record.revoked_at.map(timestamp),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = CreatedApiToken)]
pub struct CreatedApiTokenDto {
    #[serde(flatten)]
    metadata: ApiTokenDto,
    token: String,
}

impl CreatedApiTokenDto {
    pub fn new(record: &ApiTokenRecord, token: &str) -> Self {
        Self {
            metadata: ApiTokenDto::from(record),
            token: token.to_owned(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ApiTokenList)]
pub struct ApiTokenListDto {
    pub items: Vec<ApiTokenDto>,
}

#[derive(ToSchema)]
#[schema(as = ApiTokenInput)]
#[allow(dead_code)]
pub struct ApiTokenInputDoc {
    #[schema(min_length = 1, max_length = 64, example = "laptop cli")]
    name: String,
    #[schema(
        min_items = 1,
        max_items = 5,
        example = json!(["read"])
    )]
    scopes: Vec<String>,
    #[schema(min_items = 1, max_items = 50)]
    endpoint_ids: Option<Vec<String>>,
    #[schema(minimum = 1, maximum = 365, default = 90)]
    expires_in_days: Option<u32>,
}
