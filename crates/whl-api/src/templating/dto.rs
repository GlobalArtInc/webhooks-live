use serde::Serialize;
use utoipa::ToSchema;
use whl_capture::{PreviewDiagnostic, PreviewRendered};
use whl_core::validation::ValidationError;
use whl_template::{CatalogueEntry, DELIMITERS};

use crate::dto::{BodyDto, BodyInputDoc, NameValueDto};
use crate::error::FieldViolation;

#[derive(Serialize, ToSchema)]
#[schema(as = PreviewDiagnostic)]
pub struct DiagnosticDto {
    #[schema(example = "response.body")]
    field: String,
    #[schema(example = "template_invalid")]
    code: String,
    #[schema(pattern = "^(error|warning)$", example = "error")]
    severity: &'static str,
    #[schema(minimum = 1)]
    line: u32,
    #[schema(minimum = 1)]
    column: u32,
    #[schema(minimum = 1)]
    length: u32,
    message: String,
}

impl From<&PreviewDiagnostic> for DiagnosticDto {
    fn from(diagnostic: &PreviewDiagnostic) -> Self {
        Self {
            field: diagnostic.field.clone(),
            code: diagnostic.code.clone(),
            severity: diagnostic.severity.as_str(),
            line: diagnostic.line,
            column: diagnostic.column,
            length: diagnostic.length,
            message: diagnostic.message.clone(),
        }
    }
}

impl DiagnosticDto {
    pub(super) fn from_core(error: &ValidationError) -> Vec<Self> {
        error
            .violations()
            .iter()
            .map(|violation| {
                let violation = FieldViolation::from_core(violation);
                Self {
                    field: violation.field().to_owned(),
                    code: violation.code().as_str().to_owned(),
                    severity: "error",
                    line: 1,
                    column: 1,
                    length: 1,
                    message: violation.message().to_owned(),
                }
            })
            .collect()
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = RenderedResponse)]
pub struct RenderedDto {
    #[schema(minimum = 100, maximum = 599)]
    status: u16,
    content_type: String,
    headers: Vec<NameValueDto>,
    body: BodyDto,
    body_size: u64,
    duration_ms: u64,
}

impl From<&PreviewRendered> for RenderedDto {
    fn from(rendered: &PreviewRendered) -> Self {
        Self {
            status: rendered.status,
            content_type: rendered.content_type.clone(),
            headers: rendered
                .headers
                .iter()
                .map(|pair| NameValueDto::new(&pair.name, &pair.value))
                .collect(),
            body: BodyDto::from_prefix(rendered.body.as_bytes(), false),
            body_size: u64::try_from(rendered.body.len()).unwrap_or(u64::MAX),
            duration_ms: u64::try_from(rendered.elapsed.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(
    as = TemplatePreview,
    description = "`rendered` is null when the draft has error diagnostics, or when no capture was named; warnings leave it set."
)]
pub struct PreviewDto {
    capture_id: Option<String>,
    rendered: Option<RenderedDto>,
    diagnostics: Vec<DiagnosticDto>,
}

impl PreviewDto {
    pub(super) fn new(
        capture_id: Option<String>,
        rendered: Option<&PreviewRendered>,
        diagnostics: Vec<DiagnosticDto>,
    ) -> Self {
        Self {
            capture_id,
            rendered: rendered.map(RenderedDto::from),
            diagnostics,
        }
    }
}

#[derive(ToSchema)]
#[schema(as = TemplatePreviewResponseInput)]
#[allow(dead_code)]
pub struct PreviewResponseInputDoc {
    #[schema(minimum = 100, maximum = 599, default = 200)]
    status: Option<u16>,
    content_type: Option<String>,
    #[schema(max_items = 20)]
    headers: Option<Vec<NameValueDto>>,
    body: Option<BodyInputDoc>,
    #[schema(max_length = 1024)]
    status_template: Option<String>,
}

#[derive(ToSchema)]
#[schema(as = TemplatePreviewInput)]
#[allow(dead_code)]
pub struct PreviewInputDoc {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$",
        example = "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21"
    )]
    capture_id: Option<String>,
    response: PreviewResponseInputDoc,
}

#[derive(Serialize, ToSchema)]
#[schema(as = TemplateDelimiters)]
pub struct DelimitersDto {
    #[schema(example = "{{")]
    open: &'static str,
    #[schema(example = "}}")]
    close: &'static str,
}

#[derive(Serialize, ToSchema)]
#[schema(as = CatalogueVariable)]
pub struct CatalogueItemDto {
    #[schema(example = "request.query.mode")]
    path: String,
    #[schema(example = "{{ request.query.mode }}")]
    insert_text: String,
    #[schema(pattern = "^(request|time|variables|helpers)$", example = "request")]
    group: &'static str,
    #[serde(rename = "type")]
    #[schema(
        rename = "type",
        pattern = "^(string|integer|number|boolean|null|object|array|json|time|function)$",
        example = "string"
    )]
    value_type: &'static str,
    #[schema(example = "\"test\"")]
    sample: Option<String>,
    description: Option<&'static str>,
}

impl From<&CatalogueEntry> for CatalogueItemDto {
    fn from(entry: &CatalogueEntry) -> Self {
        Self {
            path: entry.path.clone(),
            insert_text: entry.insert_text.clone(),
            group: entry.group.as_str(),
            value_type: entry.value_type.as_str(),
            sample: entry.sample.clone(),
            description: entry.description,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = VariableCatalogue)]
pub struct CatalogueDto {
    delimiters: DelimitersDto,
    items: Vec<CatalogueItemDto>,
}

impl CatalogueDto {
    pub(super) fn new(entries: &[CatalogueEntry]) -> Self {
        Self {
            delimiters: DelimitersDto {
                open: DELIMITERS.open,
                close: DELIMITERS.close,
            },
            items: entries.iter().map(CatalogueItemDto::from).collect(),
        }
    }
}
