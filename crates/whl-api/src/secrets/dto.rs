use serde::Serialize;
use utoipa::ToSchema;
use whl_store::secrets::SecretMeta;

#[derive(Serialize, ToSchema)]
#[schema(as = Secret)]
pub struct SecretDto {
    #[schema(pattern = "^[A-Z][A-Z0-9_]{0,63}$", example = "CRM_TOKEN")]
    name: String,
    #[schema(read_only, format = DateTime)]
    created_at: String,
    #[schema(read_only, format = DateTime)]
    updated_at: String,
}

impl From<&SecretMeta> for SecretDto {
    fn from(meta: &SecretMeta) -> Self {
        Self {
            name: meta.name.unpack().to_owned(),
            created_at: meta.created_at.clone(),
            updated_at: meta.updated_at.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = SecretList)]
pub struct SecretListDto {
    pub items: Vec<SecretDto>,
}

#[derive(ToSchema)]
#[schema(
    as = SecretInput,
    description = "`value` is the secret, 1 to 4096 bytes. Write-only: it is never returned, not even by the listing."
)]
#[allow(dead_code)]
pub struct SecretInputDoc {
    #[schema(write_only)]
    value: String,
}
