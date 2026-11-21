mod compiled;
mod engine;
mod render;
mod report;

pub use compiled::{
    BODY_FIELD, ResponseSources, STATUS_FIELD, SourcePosition, TemplateViolation, ViolationKind,
};
pub use engine::TemplateSettings;
pub(crate) use engine::{RenderInput, TemplateEngine};
pub(crate) use report::{FallbackCode, RenderedParts, Served};

pub fn validate_response_templates(
    sources: &ResponseSources<'_>,
) -> Result<(), Vec<TemplateViolation>> {
    compiled::compile(sources).map(|_| ())
}
