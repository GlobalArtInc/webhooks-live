#![allow(dead_code)]

use utoipa::OpenApi;

use super::params::EndpointIdParam;
use super::responses::{StoreUnavailable, Unauthenticated};
use crate::error::ErrorBody;

#[utoipa::path(
    get,
    path = "/endpoints/{id}/stream",
    operation_id = "streams_endpoint",
    tag = "Streams",
    summary = "Live events of one endpoint (Server-Sent Events)",
    description = "Always starts with `ready` (`{endpoint_id, last_seq, capture_count}`), then any replay, then live events: `capture` (id = seq), `capture_deleted`, `captures_cleared`, `endpoint_updated`, `endpoint_deleted`, `forward_result`, `capture_updated`, `rule_result`, `notification_result`, `captures_imported`, `resync` (`reason`: `lagged`, `gap`, `listener_reconnect`) and `bye`. Sends `retry: 2000`, a `: ping` comment every 15 s and closes after 30 minutes. Resume with the `Last-Event-ID` header or `?last_event_id=`; when both are present the larger wins and a malformed value is ignored; up to 100 captures are replayed before `resync`. Needs a viewer on the endpoint; a token needs the `read` scope. A view-protected anonymous endpoint also needs `X-Webhooks-Live-Grant` or `?grant=`.",
    security(("session_cookie" = []), ("api_token" = []), ()),
    params(
        EndpointIdParam,
        ("last_event_id" = Option<i64>, Query, minimum = 0, description = "Resume point: the highest `capture` seq the client holds"),
        ("grant" = Option<String>, Query, description = "View grant of a protected anonymous endpoint, instead of the header"),
        ("Last-Event-ID" = Option<i64>, Header, minimum = 0, description = "Sent by `EventSource` on reconnect"),
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of a protected anonymous endpoint")
    ),
    responses(
        (status = 200, description = "The event stream", content_type = "text/event-stream", body = String),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`insufficient_scope`: the token lacks `read` or is limited to other endpoints", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
fn endpoint_stream() {}

#[utoipa::path(
    get,
    path = "/inbox/stream",
    operation_id = "streams_inbox",
    tag = "Streams",
    summary = "Live events of several endpoints at once (Server-Sent Events)",
    description = "Sends the events of `GET /endpoints/{id}/stream` for every endpoint of the selection, except `endpoint_updated`; payloads carry `endpoint_id`. Event ids are opaque; resume with `Last-Event-ID` or `?last_event_id=` replays at most 100 events. The selection is `?endpoints=` (comma-separated ids, at most 50 signed in and 20 anonymous) or, signed in, all own and shared endpoints (`endpoints` is required in `open` mode). `folder` is accepted by the contract but answers 422 until folders are selectable. Needs a viewer on every selected endpoint, else 410 `endpoint_gone`. At most 4 streams per user or address. A Bearer token is not part of the contract of this route: `GET /inbox/captures` refuses it with 403 `session_required`, and the stream does not enforce that refusal today.",
    security(("session_cookie" = []), ()),
    params(
        ("endpoints" = Option<String>, Query, description = "Comma-separated endpoint ids"),
        ("folder" = Option<String>, Query, description = "Folder id; not selectable yet"),
        ("last_event_id" = Option<String>, Query, description = "Opaque resume vector from earlier event ids"),
        ("grant" = Option<String>, Query, description = "View grant of protected anonymous endpoints, instead of the header"),
        ("Last-Event-ID" = Option<String>, Header, description = "Sent by `EventSource` on reconnect"),
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of protected anonymous endpoints")
    ),
    responses(
        (status = 200, description = "The event stream", content_type = "text/event-stream", body = String),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`sign_in_required` (`mixed`) for the default selection without an account", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: an endpoint of an explicit selection is unknown or not viewable", body = ErrorBody),
        (status = 422, description = "`validation_failed`: malformed query, malformed or too long id list, or `folder`", body = ErrorBody),
        (status = 429, description = "`rate_limited`: too many inbox streams are open (`Retry-After: 30`)", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
fn inbox_stream() {}

#[derive(OpenApi)]
#[openapi(
    paths(endpoint_stream, inbox_stream),
    tags((name = "Streams", description = "Server-Sent Events, served by whl-live and mounted beside the API router"))
)]
pub struct StreamsDoc;

#[cfg(test)]
pub(super) fn routes() -> Vec<crate::routing::RouteEntry> {
    [whl_live::STREAM_PATH, whl_live::INBOX_STREAM_PATH]
        .into_iter()
        .map(|path| crate::routing::RouteEntry {
            method: axum::http::Method::GET,
            path: path.strip_prefix("/api/v1").expect("an /api/v1 path"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, Uri, header};
    use axum::response::IntoResponse;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;
    use whl_live::{
        HUB_CAPACITY, Hub, InboxSelector, LiveState, SelectionFuture, StreamRequest, StreamSettings,
    };
    use whl_store::Store;

    use super::routes;

    const ENDPOINT: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

    #[derive(Debug)]
    struct Refusing;

    impl InboxSelector for Refusing {
        fn select<'a>(&'a self, _request: &'a StreamRequest, _uri: &'a Uri) -> SelectionFuture<'a> {
            Box::pin(async { Err(StatusCode::FORBIDDEN.into_response()) })
        }
    }

    async fn status(router: &axum::Router, path: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .body(Body::empty())
            .expect("a valid request");
        let reply = router.clone().oneshot(request).await.expect("a reply");
        let allow = reply
            .headers()
            .get(header::ALLOW)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        (reply.status(), allow)
    }

    #[tokio::test]
    async fn whl_live_serves_exactly_the_get_routes_the_coverage_test_adds() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://stream:stream@127.0.0.1:1/stream")
            .expect("a lazy pool");
        let state = LiveState::new(
            Store::from_pool(pool),
            Hub::new(HUB_CAPACITY),
            StreamSettings::new(Duration::from_secs(60)),
        )
        .with_inbox(Arc::new(Refusing));
        let router = whl_live::router(state);
        let expected = routes();
        assert_eq!(expected.len(), 2);
        for entry in expected {
            assert_eq!(entry.method, Method::GET);
            let path = format!("/api/v1{}", entry.path.replace("{id}", ENDPOINT));
            let (code, allow) = status(&router, &path).await;
            assert_eq!(code, StatusCode::METHOD_NOT_ALLOWED, "{path}");
            assert!(allow.contains("GET"), "{path}: {allow}");
        }
        let (code, _) = status(&router, "/api/v1/endpoints/x/streams").await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }
}
