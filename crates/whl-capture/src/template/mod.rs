mod compiled;
mod engine;
mod preview;
mod render;
mod report;

pub use compiled::{
    BODY_FIELD, ResponseSources, STATUS_FIELD, SourcePosition, TemplateViolation, ViolationKind,
};
pub use engine::TemplateSettings;
pub(crate) use engine::{RenderInput, TemplateEngine};
pub use preview::{
    PreviewDiagnostic, PreviewRendered, PreviewReport, PreviewSeverity, TemplateDraft,
    persistent_context, render_preview, response_context,
};
pub(crate) use report::{FallbackCode, RenderedParts, Served};

pub fn validate_response_templates(
    sources: &ResponseSources<'_>,
) -> Result<(), Vec<TemplateViolation>> {
    compiled::compile(sources).map(|_| ())
}
