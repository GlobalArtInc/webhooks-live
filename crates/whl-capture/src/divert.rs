use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tower::ServiceExt;

use crate::handler::endpoint_id_of;

pub(crate) async fn divert(
    State(fallback): State<Router>,
    request: Request,
    next: Next,
) -> Response {
    if endpoint_id_of(request.uri()).is_some() {
        return next.run(request).await;
    }
    match fallback.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}
