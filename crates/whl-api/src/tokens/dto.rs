use serde::Serialize;
use whl_store::ApiTokenRecord;

use crate::dto::timestamp;

#[derive(Serialize)]
pub struct ApiTokenDto {
    id: String,
    name: String,
    prefix: String,
    scopes: Vec<&'static str>,
    endpoint_ids: Option<Vec<String>>,
    created_at: String,
    expires_at: String,
    last_used_at: Option<String>,
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

#[derive(Serialize)]
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

#[derive(Serialize)]
pub struct ApiTokenListDto {
    pub items: Vec<ApiTokenDto>,
}
