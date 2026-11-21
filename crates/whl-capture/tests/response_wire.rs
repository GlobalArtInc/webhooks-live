mod common;
mod served;

use std::time::Duration;

use common::Harness;
use served::{Configured, Wire, endpoint_configured, serve};
use whl_core::endpoint_id::EndpointId;

const QUIET: Duration = Duration::from_millis(300);

async fn get(wire: &mut Wire, id: &EndpointId, suffix: &str, method: &str) {
    wire.write(format!("{method} /{id}{suffix} HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
        .await;
}

async fn assert_connection_reusable(wire: &mut Wire, id: &EndpointId) {
    get(wire, id, "/next", "GET").await;
    let head = wire.head().await;
    assert_eq!(head.status, 200);
    let length: usize = head
        .header("content-length")
        .and_then(|value| value.parse().ok())
        .expect("content length");
    wire.body(length).await;
}

#[tokio::test]
async fn ordinary_response_is_framed_by_content_length() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"hello".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    get(&mut wire, &id, "", "GET").await;
    let head = wire.head().await;
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length").as_deref(), Some("5"));
    assert!(head.header("transfer-encoding").is_none());
    assert_eq!(wire.body(5).await, b"hello");
    assert_connection_reusable(&mut wire, &id).await;
}

#[tokio::test]
async fn informational_status_is_never_sent_as_a_final_response() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"final".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    for code in [100, 101, 102, 199] {
        wire.write(
            format!(
                "GET /{id}/{code} HTTP/1.1\r\nHost: t\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n"
            )
            .as_bytes(),
        )
        .await;
        let head = wire.head().await;
        assert!(
            head.text.starts_with("HTTP/1.1 200 "),
            "{code}: {}",
            head.text
        );
        assert_eq!(
            head.header("x-webhooks-live-substituted-status").as_deref(),
            Some(code.to_string().as_str())
        );
        assert!(head.header("upgrade").is_none());
        assert_eq!(head.header("content-length").as_deref(), Some("5"));
        assert_eq!(wire.body(5).await, b"final");
    }
    assert_connection_reusable(&mut wire, &id).await;
}

#[tokio::test]
async fn no_content_statuses_are_sent_without_body_or_body_headers() {
    let harness = Harness::new().await;
    let addr = serve(&harness).await;

    for (status, reason) in [
        (204_i16, "No Content"),
        (205, "Reset Content"),
        (304, "Not Modified"),
    ] {
        let id = endpoint_configured(
            &harness,
            Configured {
                status,
                body: b"ignored".to_vec(),
                ..Configured::default()
            },
        )
        .await;
        let mut wire = Wire::connect(addr).await;

        get(&mut wire, &id, "", "GET").await;
        let head = wire.head().await;
        assert!(
            head.text
                .starts_with(&format!("HTTP/1.1 {status} {reason}")),
            "{}",
            head.text
        );
        assert!(head.header("content-type").is_none(), "{status}");
        assert!(head.header("transfer-encoding").is_none(), "{status}");
        let expected_length = (status == 205).then(|| "0".to_owned());
        assert_eq!(head.header("content-length"), expected_length, "{status}");
        assert!(wire.leftover_after(QUIET).await.is_empty(), "{status}");

        get(&mut wire, &id, "", "GET").await;
        let again = wire.head().await;
        assert_eq!(again.status, status as u16);
    }
}

#[tokio::test]
async fn overridden_no_content_status_keeps_the_connection_framed() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"body".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    get(&mut wire, &id, "/204", "GET").await;
    let head = wire.head().await;
    assert_eq!(head.status, 204);
    assert!(head.header("content-length").is_none());
    assert!(wire.leftover_after(QUIET).await.is_empty());
    assert_connection_reusable(&mut wire, &id).await;
}

#[tokio::test]
async fn head_sends_the_get_headers_and_no_body() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            content_type: "text/html",
            body: b"<b>hello</b>".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let addr = serve(&harness).await;
    let mut wire = Wire::connect(addr).await;

    get(&mut wire, &id, "", "GET").await;
    let get_head = wire.head().await;
    assert_eq!(wire.body(12).await, b"<b>hello</b>");

    get(&mut wire, &id, "", "HEAD").await;
    let head = wire.head().await;
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length").as_deref(), Some("12"));
    assert_eq!(head.header("content-type").as_deref(), Some("text/html"));
    assert!(wire.leftover_after(QUIET).await.is_empty());

    let comparable = |names: Vec<String>| {
        names
            .into_iter()
            .filter(|name| name != "date")
            .collect::<Vec<_>>()
    };
    assert_eq!(comparable(head.names()), comparable(get_head.names()));
    assert_connection_reusable(&mut wire, &id).await;
}
