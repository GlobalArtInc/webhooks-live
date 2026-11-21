use serde::Serialize;
use serde_json::Value;
use whl_store::vars::VarRecord;

#[derive(Serialize)]
pub struct VarDto {
    name: String,
    value: Value,
    updated_at: String,
}

impl From<&VarRecord> for VarDto {
    fn from(record: &VarRecord) -> Self {
        Self {
            name: record.name.clone(),
            value: record.value.clone(),
            updated_at: record.updated_at.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct VarListDto {
    pub items: Vec<VarDto>,
}
