use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{MatchedPath, State};
use axum::http::{Request, StatusCode, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::trace::TraceLayer;
use tracing::Span;
use whl_api::{ApiLimits, ApiState, enforce_rate_limit};
use whl_auth::AuthState;
use whl_capture::CaptureState;
use whl_live::LiveState;

use crate::readiness::Readiness;

pub fn public(
    live: LiveState,
    api: ApiState,
    capture: CaptureState,
    auth: Option<AuthState>,
    limits: ApiLimits,
    api_cors: bool,
) -> Router {
    let ingest = whl_capture::router_with_fallback(capture, whl_web::router());
    let stream = whl_api::guard_routes(whl_live::router(live), api.access())
        .layer(from_fn_with_state(limits.clone(), enforce_rate_limit));
    let routes = Router::new().merge(stream);
    let routes = match auth {
        Some(auth) => routes
            .merge(whl_api::router(api).layer(auth.origin_check()))
            .merge(whl_auth::router(auth).layer(from_fn_with_state(limits, enforce_rate_limit))),
        None => routes
            .merge(whl_api::router(api))
            .merge(whl_auth::unavailable_router()),
    };
    let routes = if api_cors {
        routes.layer(from_fn(whl_api::api_cors))
    } else {
        routes
    };
    with_tracing(routes.fallback_service(ingest))
}

pub fn admin(readiness: Arc<Readiness>) -> Router {
    let routes = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .with_state(readiness)
        .fallback(not_found);
    with_tracing(routes)
}

fn with_tracing(router: Router) -> Router {
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|request: &Request<Body>| {
                let route = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map_or("fallback", MatchedPath::as_str);
                tracing::info_span!("http_request", method = %request.method(), route)
            })
            .on_response(|response: &Response, latency: Duration, span: &Span| {
                let status = response.status().as_u16();
                let duration_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX);
                span.in_scope(|| tracing::info!(status, duration_ms, "request finished"));
            }),
    )
}

async fn not_found() -> (StatusCode, &'static str) {
    (StatusCode::NOT_FOUND, "not found")
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(State(readiness): State<Arc<Readiness>>) -> (StatusCode, &'static str) {
    if readiness.is_ready() {
        if readiness.auth_unavailable() {
            (StatusCode::OK, "ok, sign-in unavailable")
        } else {
            (StatusCode::OK, "ok")
        }
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

async fn metrics() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        whl_telemetry::metrics::render(),
    )
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tower::ServiceExt;
    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    const GRANT: &str = "sentinel-grant-for-the-log-test";

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Captured {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("the log buffer is poisoned"))?
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[tokio::test]
    async fn request_logs_never_carry_the_query_string_of_a_stream_request() {
        let captured = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(captured.clone())
            .finish();
        let _installed = tracing::subscriber::set_default(subscriber);
        let app = with_tracing(
            Router::new().route("/api/v1/endpoints/{id}/stream", get(|| async { "ok" })),
        );
        let request = Request::builder()
            .uri(format!(
                "/api/v1/endpoints/abc/stream?grant={GRANT}&last_event_id=4"
            ))
            .body(Body::empty())
            .expect("the request is valid");

        let response = app
            .oneshot(request)
            .await
            .expect("the router does not fail");

        assert_eq!(response.status(), StatusCode::OK);
        let logs = String::from_utf8_lossy(&captured.0.lock().expect("the buffer is readable"))
            .into_owned();
        assert!(logs.contains("request finished"), "{logs}");
        assert!(!logs.contains(GRANT), "{logs}");
        assert!(!logs.contains("last_event_id"), "{logs}");
    }
}
