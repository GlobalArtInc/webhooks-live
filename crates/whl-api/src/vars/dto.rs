use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use whl_store::vars::VarRecord;

#[derive(Serialize, ToSchema)]
#[schema(
    as = Var,
    description = "`value` is any JSON value; counters are variables holding an integer."
)]
pub struct VarDto {
    #[schema(pattern = "^[a-z_][a-z0-9_]{0,63}$", example = "refund_count")]
    name: String,
    #[schema(example = 3)]
    value: Value,
    #[schema(read_only, format = DateTime)]
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

#[derive(Serialize, ToSchema)]
#[schema(as = VarList)]
pub struct VarListDto {
    pub items: Vec<VarDto>,
}

#[derive(ToSchema)]
#[schema(
    as = VarInput,
    description = "`value` is any JSON value, at most 4 KiB serialized, with no NUL character anywhere in it."
)]
#[allow(dead_code)]
pub struct VarInputDoc {
    value: Value,
}
