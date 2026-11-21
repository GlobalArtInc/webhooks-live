mod body;
mod client_ip;
mod divert;
mod form;
mod handler;
mod refusal;
mod response;
mod seams;
mod state;

use axum::Router;
use axum::middleware::from_fn_with_state;
use axum::routing::any;

pub use body::{BufferedBody, ReadFailure, read_body};
pub use client_ip::{ForwardedRequest, Scheme, TrustedProxies};
pub use form::{FormLimits, parse_form, parse_query};
pub use response::build_response;
pub use seams::{Admission, StorageFull};
pub use state::{
    BodyBudget, BudgetExhausted, CaptureLimits, CaptureSettings, CaptureSource, CaptureState,
    StateError, SystemSource,
};

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
