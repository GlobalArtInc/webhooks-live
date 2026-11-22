use serde_json::Value;
use utoipa::{OpenApi, ToSchema};

use super::{handlers, templates};

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::clone_endpoint,
        templates::list,
        templates::create,
        templates::show,
        templates::rename,
        templates::destroy
    ),
    components(schemas(
        CloneInputDoc,
        CloneResultDoc,
        TemplateInputDoc,
        TemplatePatchInputDoc,
        templates::TemplateDto,
        templates::TemplateSummaryDto,
        templates::TemplateListDto
    )),
    tags((name = "Cloning", description = "Copies of an endpoint and reusable configuration templates"))
)]
pub struct CloningDoc;

#[derive(ToSchema)]
#[schema(as = CloneInput)]
#[allow(dead_code)]
pub struct CloneInputDoc {
    #[schema(max_length = 100, example = "Staging copy")]
    title: Option<String>,
    #[schema(example = json!(["response", "rules"]))]
    include: Option<Vec<String>>,
}

#[derive(ToSchema)]
#[schema(as = CloneResult)]
#[allow(dead_code)]
pub struct CloneResultDoc {
    #[serde(flatten)]
    #[schema(value_type = Object)]
    endpoint: Value,
    #[schema(example = json!(["WEBHOOK_SECRET"]))]
    secrets_missing: Vec<String>,
    #[schema(example = json!(["channels"]))]
    skipped: Vec<String>,
}

#[derive(ToSchema)]
#[schema(as = TemplateInput)]
#[allow(dead_code)]
pub struct TemplateInputDoc {
    #[schema(min_length = 1, max_length = 64, example = "Staging")]
    name: String,
    #[schema(
        min_length = 24,
        max_length = 24,
        pattern = "^[0-9a-hjkmnp-tv-z]{24}$",
        example = "7k2m9q4xw8c1d5f3h6j0n2p4"
    )]
    from_endpoint_id: String,
}

#[derive(ToSchema)]
#[schema(as = TemplatePatchInput)]
#[allow(dead_code)]
pub struct TemplatePatchInputDoc {
    #[schema(min_length = 1, max_length = 64, example = "Staging v2")]
    name: String,
}
