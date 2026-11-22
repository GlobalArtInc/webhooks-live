use utoipa::{IntoParams, OpenApi};

use super::dto::{
    CatalogueDto, CatalogueItemDto, DelimitersDto, DiagnosticDto, PreviewDto, PreviewInputDoc,
    PreviewResponseInputDoc, RenderedDto,
};
use super::{preview, variables};

#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
#[allow(dead_code)]
pub struct VariablesQuery {
    #[param(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
        example = "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21"
    )]
    capture_id: Option<String>,
}

#[derive(OpenApi)]
#[openapi(
    paths(preview::preview, variables::variables),
    components(schemas(
        PreviewDto,
        PreviewInputDoc,
        PreviewResponseInputDoc,
        RenderedDto,
        DiagnosticDto,
        CatalogueDto,
        CatalogueItemDto,
        DelimitersDto
    )),
    tags((name = "Templating", description = "Response templating: the variable catalogue of a capture and a server-side preview of a draft response"))
)]
pub struct TemplatingDoc;
