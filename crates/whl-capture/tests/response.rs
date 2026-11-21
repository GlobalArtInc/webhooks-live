mod common;
mod served;

use axum::body::Body;
use common::{Harness, request};
use served::{Configured, endpoint_configured, expected, sorted_pairs};
use whl_core::endpoint_id::EndpointId;

const SAFETY: [(&str, &str); 3] = [
    ("content-security-policy", "sandbox"),
    ("x-content-type-options", "nosniff"),
    ("cache-control", "no-store"),
];
const CORS: [(&str, &str); 2] = [
    ("access-control-allow-origin", "*"),
    (
        "access-control-expose-headers",
        "X-Request-Id, X-Endpoint-Id",
    ),
];

async fn options(harness: &Harness, id: &EndpointId, headers: &[(&str, &str)]) -> common::Reply {
    let mut builder = request("OPTIONS", &format!("/{id}/hook"));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    harness
        .send(builder.body(Body::empty()).expect("request builds"))
        .await
}

fn with_ids(id: &EndpointId, request_id: &str, rest: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut pairs: Vec<(&str, &str)> =
        vec![("x-request-id", request_id), ("x-endpoint-id", id.unpack())];
    pairs.extend(SAFETY);
    pairs.extend(rest.iter().copied());
    expected(&pairs)
}

fn request_id_of(reply: &common::Reply) -> String {
    reply
        .header("x-request-id")
        .expect("request id header")
        .to_owned()
}

#[tokio::test]
async fn default_endpoint_answers_with_the_exact_header_set() {
    let harness = Harness::new().await;
    let id = endpoint_configured(&harness, Configured::default()).await;

    let reply = harness.post(&format!("/{id}"), "payload").await;
    let stored = &harness.rows(&id).await[0];
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "");
    assert_eq!(request_id_of(&reply), stored.id);
    assert_eq!(
        sorted_pairs(&reply.headers),
        with_ids(
            &id,
            &stored.id,
            &[("content-type", "text/plain"), ("content-length", "0")]
        )
    );
}

#[tokio::test]
async fn custom_endpoint_answers_with_the_exact_header_set() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            content_type: "application/json; charset=utf-8",
            headers: vec![
                ("X-Custom", "a b"),
                ("Link", "<https://x.test/2>; rel=next"),
            ],
            body: br#"{"ok":true}"#.to_vec(),
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.get(&format!("/{id}/path")).await;
    let stored = &harness.rows(&id).await[0];
    assert_eq!((reply.status, reply.body.as_str()), (201, r#"{"ok":true}"#));
    assert_eq!(
        sorted_pairs(&reply.headers),
        with_ids(
            &id,
            &stored.id,
            &[
                ("content-type", "application/json; charset=utf-8"),
                ("content-length", "11"),
                ("x-custom", "a b"),
                ("link", "<https://x.test/2>; rel=next"),
            ]
        )
    );
}

#[tokio::test]
async fn status_override_beats_the_configured_status() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 201,
            body: b"configured".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    for (path, status) in [
        (format!("/{id}/404"), 404),
        (format!("/{id}/503/deeper"), 503),
        (format!("/{id}/99"), 201),
        (format!("/{id}/600"), 201),
        (format!("/{id}/x/404"), 201),
    ] {
        let reply = harness.get(&path).await;
        assert_eq!(reply.status, status, "{path}");
        assert_eq!(reply.body, "configured", "{path}");
        assert_eq!(reply.header("content-type"), Some("text/plain"), "{path}");
    }
}

#[tokio::test]
async fn informational_statuses_are_answered_with_200_and_a_marker() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"final".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    for code in [100, 101, 102, 199] {
        let reply = harness.get(&format!("/{id}/{code}")).await;
        assert_eq!(reply.status, 200, "{code}");
        assert_eq!(reply.body, "final", "{code}");
        assert_eq!(
            reply.header("x-webhooks-live-substituted-status"),
            Some(code.to_string().as_str()),
            "{code}"
        );
    }
    let reply = harness.get(&format!("/{id}/200")).await;
    assert!(reply.header("x-webhooks-live-substituted-status").is_none());

    let configured = endpoint_configured(
        &harness,
        Configured {
            status: 103,
            ..Configured::default()
        },
    )
    .await;
    let reply = harness.get(&format!("/{configured}")).await;
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.header("x-webhooks-live-substituted-status"),
        Some("103")
    );
}

#[tokio::test]
async fn reserved_marker_headers_cannot_be_forged_by_configuration() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            headers: vec![
                ("X-Webhooks-Live-Substituted-Status", "999"),
                ("X-Webhooks-Live-Error", "endpoint_gone"),
            ],
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.get(&format!("/{id}")).await;
    assert_eq!(reply.status, 200);
    assert!(reply.header("x-webhooks-live-substituted-status").is_none());
    assert!(reply.header("x-webhooks-live-error").is_none());

    let reply = harness.get(&format!("/{id}/101")).await;
    let values: Vec<_> = reply
        .headers
        .get_all("x-webhooks-live-substituted-status")
        .iter()
        .collect();
    assert_eq!(values, ["101"]);
}

#[tokio::test]
async fn a_stored_response_that_fails_validation_is_never_served() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            headers: vec![("Set-Cookie", "__Host-whl_session=stolen")],
            body: b"<script>1</script>".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.get(&format!("/{id}")).await;
    assert_eq!((reply.status, reply.body.as_str()), (500, ""));
    assert!(reply.header("set-cookie").is_none());
    assert_eq!(
        sorted_pairs(&reply.headers),
        with_ids(
            &id,
            &harness.rows(&id).await[0].id,
            &[("content-length", "0")]
        )
    );
    assert_eq!(harness.rows(&id).await.len(), 1);
}

#[tokio::test]
async fn no_content_statuses_have_no_body_and_no_body_headers() {
    let harness = Harness::new().await;
    for status in [204_i16, 205, 304] {
        let id = endpoint_configured(
            &harness,
            Configured {
                status,
                body: b"ignored".to_vec(),
                headers: vec![("ETag", "\"v1\"")],
                ..Configured::default()
            },
        )
        .await;
        let reply = harness.get(&format!("/{id}")).await;
        assert_eq!(reply.status, status as u16);
        assert_eq!(reply.body, "");
        assert_eq!(
            reply.header("content-length"),
            (status == 205).then_some("0"),
            "{status}"
        );
        assert!(reply.header("content-type").is_none(), "{status}");
        assert_eq!(reply.header("etag"), Some("\"v1\""));
    }
}

#[tokio::test]
async fn head_answers_the_headers_of_a_get_without_a_body() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            content_type: "text/html",
            body: b"<b>hi</b>".to_vec(),
            headers: vec![("X-Custom", "1")],
            ..Configured::default()
        },
    )
    .await;

    let get = harness.get(&format!("/{id}")).await;
    let head = harness
        .send(
            request("HEAD", &format!("/{id}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await;
    assert_eq!(head.status, 200);
    assert_eq!(head.body, "");
    assert_eq!(head.header("content-length"), Some("9"));
    let strip = |reply: &common::Reply| {
        sorted_pairs(&reply.headers)
            .into_iter()
            .filter(|(name, _)| name != "x-request-id")
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&head), strip(&get));
}

#[tokio::test]
async fn cors_off_adds_no_cors_headers_and_options_is_an_ordinary_capture() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 202,
            body: b"plain".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    let reply = options(
        &harness,
        &id,
        &[
            ("origin", "https://app.test"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "x-a"),
        ],
    )
    .await;
    assert_eq!((reply.status, reply.body.as_str()), (202, "plain"));
    assert!(
        reply
            .headers
            .keys()
            .all(|name| !name.as_str().starts_with("access-control-"))
    );
    assert_eq!(harness.rows(&id).await[0].method, "OPTIONS");
}

#[tokio::test]
async fn cors_on_adds_the_permissive_headers_to_every_response() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            cors: true,
            body: b"x".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    let reply = harness.post(&format!("/{id}/404"), "payload").await;
    let stored = &harness.rows(&id).await[0];
    assert_eq!(reply.status, 404);
    assert_eq!(
        sorted_pairs(&reply.headers),
        with_ids(
            &id,
            &stored.id,
            &[
                ("content-type", "text/plain"),
                ("content-length", "1"),
                CORS[0],
                CORS[1],
            ]
        )
    );
}

#[tokio::test]
async fn preflight_is_captured_and_answered_204_with_echoed_request() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 418,
            cors: true,
            body: b"configured".to_vec(),
            headers: vec![("X-Custom", "1")],
            ..Configured::default()
        },
    )
    .await;

    let reply = options(
        &harness,
        &id,
        &[
            ("origin", "https://app.test"),
            ("access-control-request-method", "PUT"),
            ("access-control-request-headers", "content-type, X-Api-Key"),
        ],
    )
    .await;
    let rows = harness.rows(&id).await;
    assert_eq!((reply.status, reply.body.as_str()), (204, ""));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].method, "OPTIONS");
    assert_eq!(
        sorted_pairs(&reply.headers),
        with_ids(
            &id,
            &rows[0].id,
            &[
                CORS[0],
                CORS[1],
                ("access-control-allow-methods", "PUT"),
                ("access-control-allow-headers", "content-type, X-Api-Key"),
                ("access-control-max-age", "600"),
            ]
        )
    );
}

#[tokio::test]
async fn preflight_filters_requested_header_names_and_caps_their_number() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            cors: true,
            ..Configured::default()
        },
    )
    .await;

    let reply = options(
        &harness,
        &id,
        &[
            ("origin", "https://app.test"),
            ("access-control-request-method", "POST"),
            (
                "access-control-request-headers",
                "x-ok,  bad name , <script>, , x-two\t, a/b",
            ),
        ],
    )
    .await;
    assert_eq!(
        reply.header("access-control-allow-headers"),
        Some("x-ok, x-two")
    );

    let many: Vec<String> = (0..50).map(|index| format!("x-h{index}")).collect();
    let many = many.join(",");
    let reply = options(
        &harness,
        &id,
        &[
            ("origin", "https://app.test"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", many.as_str()),
        ],
    )
    .await;
    let echoed = reply
        .header("access-control-allow-headers")
        .expect("allow headers");
    assert_eq!(echoed.split(", ").count(), 20);
    assert!(echoed.starts_with("x-h0, x-h1, "));

    let long = "h".repeat(65);
    let reply = options(
        &harness,
        &id,
        &[
            ("origin", "https://app.test"),
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", long.as_str()),
        ],
    )
    .await;
    assert_eq!(reply.status, 204);
    assert!(reply.header("access-control-allow-headers").is_none());
}

#[tokio::test]
async fn options_that_is_not_a_preflight_gets_the_configured_response_with_cors() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            status: 202,
            cors: true,
            body: b"configured".to_vec(),
            ..Configured::default()
        },
    )
    .await;

    for headers in [
        vec![],
        vec![("origin", "https://app.test")],
        vec![("access-control-request-method", "POST")],
        vec![
            ("origin", "https://app.test"),
            ("access-control-request-method", "PO ST"),
        ],
        vec![
            ("origin", "https://app.test"),
            ("access-control-request-method", ""),
        ],
    ] {
        let reply = options(&harness, &id, &headers).await;
        assert_eq!(reply.status, 202, "{headers:?}");
        assert_eq!(reply.body, "configured", "{headers:?}");
        assert_eq!(reply.header("access-control-allow-origin"), Some("*"));
        assert!(reply.header("access-control-allow-methods").is_none());
        assert!(reply.header("access-control-max-age").is_none());
    }
}
