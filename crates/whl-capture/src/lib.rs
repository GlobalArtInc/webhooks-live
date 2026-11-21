mod body;
mod client_ip;
mod delay;
mod divert;
mod form;
mod handler;
mod refusal;
mod response;
mod rules;
mod seams;
mod state;
mod template;
mod verification;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::any;

pub use body::{BufferedBody, ReadFailure, read_body};
pub use client_ip::{ForwardedRequest, Scheme, TrustedProxies};
pub use delay::{DELAY_SKIPPED_TOTAL, DelaySlots};
pub use form::{FormLimits, parse_form, parse_query};
pub use response::{ReplyRequest, build_response};
pub use rules::{
    DEFAULT_DRY_RUN_CONCURRENCY, DEFAULT_PER_ENDPOINT, DEFAULT_SYNC_BUDGET,
    DEFAULT_SYNC_CONCURRENCY, DEFAULT_VARIABLES_DEADLINE, DryRunError, DryRunInput, DryRunRules,
    MAX_SYNC_BUDGET, RULE_BODY_BYTES, RuleFacts, RulesError, RulesRuntime, RulesSettings,
};
pub use seams::{Admission, StorageFull};
pub use state::{
    BodyBudget, BudgetExhausted, CaptureLimits, CaptureSettings, CaptureSource, CaptureState,
    StateError, SystemSource,
};
pub use template::{
    BODY_FIELD, ResponseSources, STATUS_FIELD, SourcePosition, TemplateSettings, TemplateViolation,
    ViolationKind, validate_response_templates,
};
pub use verification::INLINE_VERIFY_BYTES;

pub fn router(state: CaptureState) -> Router {
    let plain = Router::new()
        .fallback(handler::capture)
        .with_state(state.clone());
    router_with_fallback(state, plain)
}

pub fn router_with_fallback(state: CaptureState, fallback: Router) -> Router {
    Router::new()
        .route("/{id}", any(handler::capture))
        .route("/{id}/", any(handler::capture))
        .route("/{id}/{*rest}", any(handler::capture))
        .route_layer(from_fn_with_state(fallback.clone(), divert::divert))
        .fallback_service(fallback)
        .with_state(state)
}
