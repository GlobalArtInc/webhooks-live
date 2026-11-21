mod ast;
mod budget;
mod context;
mod env;
mod error;
mod filters;
mod format;
mod jsonpath;
mod limits;
mod parser;
mod render;
mod span;
mod target;
mod value;
mod writer;

pub use ast::Template;
pub use budget::ResponseBudget;
pub use context::{
    BodyData, CaptureData, Context, EndpointData, NameValue, RequestData, Secrets, SignatureState,
    StepData,
};
pub use env::{
    ChaChaRandom, MonotonicClock, RandomSeed, RandomSeedError, RandomSource, RenderEnv, SystemClock,
};
pub use error::{Diagnostic, ErrorCode, RenderError, RenderErrorCode, TemplateError};
pub use limits::{
    BODY_TEXT_BYTES, JSON_BODY_BYTES, JSON_BODY_DEFAULT_BYTES, JSON_BODY_DEPTH, JSON_NODES_DEFAULT,
    MAX_ARGUMENTS, MAX_DIAGNOSTICS, MAX_EXPRESSIONS, MAX_FILTERS_PER_EXPRESSION, MAX_LITERAL_BYTES,
    MAX_PATH_SEGMENTS, MAX_SOURCE_BYTES, ParseLimits, RenderLimits,
};
pub use render::{MissingVariable, PartialRender, RenderOutcome, Rendered};
pub use span::Position;
pub use target::{Policy, RenderTarget, SecretPolicy, StepPolicy};

impl Template {
    pub fn parse(source: &str, policy: &Policy) -> Result<Self, TemplateError> {
        parser::parse(source, policy)
    }

    pub fn render(
        &self,
        context: &Context,
        limits: &RenderLimits,
        env: &mut RenderEnv<'_>,
    ) -> RenderOutcome {
        render::render(self, context, limits, env, None)
    }

    pub fn render_within(
        &self,
        context: &Context,
        limits: &RenderLimits,
        env: &mut RenderEnv<'_>,
        budget: &ResponseBudget,
    ) -> RenderOutcome {
        render::render(self, context, limits, env, Some(budget))
    }
}

pub fn validate(source: &str, policy: &Policy) -> Result<(), TemplateError> {
    parser::parse(source, policy).map(|_| ())
}
