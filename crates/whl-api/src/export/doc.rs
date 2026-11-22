use utoipa::{OpenApi, ToSchema};

use super::handlers;

#[derive(ToSchema)]
#[schema(as = ExportFormat)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum ExportFormatDoc {
    Json,
    Ndjson,
    Csv,
    Zip,
}

#[derive(ToSchema)]
#[schema(as = ExportRequest)]
#[allow(dead_code)]
pub struct ExportRequestDoc {
    format: ExportFormatDoc,
    #[schema(min_items = 1, max_items = 500, example = json!(["0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"]))]
    capture_ids: Vec<String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(handlers::get, handlers::post, handlers::head),
    components(schemas(ExportFormatDoc, ExportRequestDoc)),
    tags((name = "Export", description = "Download the captures of an endpoint as JSON, NDJSON, CSV or ZIP; session only"))
)]
pub struct ExportDoc;
