mod common;

use axum::Router;
use axum::body::Body;
use axum::http::StatusCode;
use axum::routing::get;
use common::{Harness, request, send};
use whl_capture::router_with_fallback;

const STUB: &str = "ui fallback";

fn stub() -> Router {
    Router::new().fallback(get(|| async { (StatusCode::IM_A_TEAPOT, STUB) }))
}

#[tokio::test]
async fn a_valid_endpoint_id_captures_and_never_reaches_the_fallback() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let app = router_with_fallback(harness.state.clone(), stub());

    let reply = send(
        &app,
        request("POST", &format!("/{id}/hook"))
            .body(Body::from("payload"))
            .expect("request builds"),
    )
    .await;

    assert_eq!(reply.status, 200);
    assert_ne!(reply.body, STUB);
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn an_invalid_first_segment_is_handed_to_the_fallback_and_stores_nothing() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let valid = id.unpack().to_owned();
    let app = router_with_fallback(harness.state.clone(), stub());
    let cases = [
        valid.to_uppercase(),
        valid[..23].to_owned(),
        format!("{valid}a"),
        format!("{}i", &valid[..23]),
        "settings".to_owned(),
    ];

    for case in &cases {
        for path in [
            format!("/{case}"),
            format!("/{case}/"),
            format!("/{case}/deeper/path"),
        ] {
            let reply = send(
                &app,
                request("GET", &path)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await;
            assert_eq!((reply.status, reply.body.as_str()), (418, STUB), "{path}");
            assert_eq!(reply.header("x-webhooks-live-error"), None, "{path}");
        }
    }

    let root = send(
        &app,
        request("GET", "/")
            .body(Body::empty())
            .expect("request builds"),
    )
    .await;
    assert_eq!((root.status, root.body.as_str()), (418, STUB));
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn the_fallback_receives_the_original_method_and_body() {
    let harness = Harness::new().await;
    let echo = Router::new().fallback(|method: axum::http::Method, body: String| async move {
        format!("{method} {body}")
    });
    let app = router_with_fallback(harness.state.clone(), echo);

    let reply = send(
        &app,
        request("PUT", "/settings/x")
            .body(Body::from("forwarded"))
            .expect("request builds"),
    )
    .await;

    assert_eq!((reply.status, reply.body.as_str()), (200, "PUT forwarded"));
}
