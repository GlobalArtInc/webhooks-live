use serde::Serialize;
use whl_store::secrets::SecretMeta;

#[derive(Serialize)]
pub struct SecretDto {
    name: String,
    created_at: String,
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

#[derive(Serialize)]
pub struct SecretListDto {
    pub items: Vec<SecretDto>,
}
