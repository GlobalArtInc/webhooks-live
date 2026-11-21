mod common;

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::{Duration, Instant};

use axum::body::Body;
use bytes::Bytes;
use common::{Harness, default_limits, request};
use futures_util::stream;
use tokio::sync::mpsc;
use whl_capture::CaptureLimits;

fn channel_body() -> (mpsc::Sender<Bytes>, Body) {
    let (sender, receiver) = mpsc::channel::<Bytes>(8);
    let source = stream::unfold(receiver, |mut receiver| async move {
        receiver
            .recv()
            .await
            .map(|chunk| (Ok::<_, Infallible>(chunk), receiver))
    });
    (sender, Body::from_stream(source))
}

fn small_limits() -> CaptureLimits {
    CaptureLimits {
        max_body_bytes: 1000,
        ..default_limits()
    }
}

#[tokio::test]
async fn content_length_over_the_limit_answers_413_before_reading() {
    let harness = Harness::with_limits(small_limits()).await;
    let id = harness.endpoint().await;
    let (_sender, body) = channel_body();
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-length", "5000")
                .body(body)
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 413);
    assert_eq!(reply.body, "Request body exceeds 1000 bytes.");
    assert_eq!(reply.header("x-webhooks-live-error"), Some("too_large"));
    assert_eq!(reply.header("connection"), Some("close"));
    assert!(harness.rows(&id).await.is_empty());
    assert_eq!(harness.state.budget().in_use(), 0);
}

#[tokio::test]
async fn stream_overflow_without_content_length_answers_413() {
    let harness = Harness::with_limits(small_limits()).await;
    let id = harness.endpoint().await;
    let chunks = vec![Ok::<_, Infallible>(Bytes::from(vec![b'x'; 600])); 2];
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .body(Body::from_stream(stream::iter(chunks)))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 413);
    assert_eq!(reply.header("connection"), Some("close"));
    assert!(harness.rows(&id).await.is_empty());
    assert_eq!(harness.state.budget().in_use(), 0);
}

#[tokio::test]
async fn body_at_exactly_the_limit_is_accepted() {
    let harness = Harness::with_limits(small_limits()).await;
    let id = harness.endpoint().await;
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .body(Body::from(vec![b'y'; 1000]))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert_eq!(harness.rows(&id).await[0].body_size, 1000);
    assert_eq!(harness.state.budget().in_use(), 0);
}

#[tokio::test]
async fn exhausted_memory_budget_answers_503_until_bytes_are_released() {
    let harness = Harness::with_limits(CaptureLimits {
        body_memory_budget: 4000,
        budget_wait: Duration::from_millis(150),
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;

    let (sender, body) = channel_body();
    let router = harness.router.clone();
    let holder = tokio::spawn({
        let uri = format!("/{id}");
        async move {
            common::send(
                &router,
                request("POST", &uri)
                    .header("content-length", "3000")
                    .body(body)
                    .expect("request builds"),
            )
            .await
        }
    });
    sender
        .send(Bytes::from_static(b"0123456789"))
        .await
        .expect("holder reads");
    while harness.state.budget().in_use() == 0 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(harness.state.budget().in_use(), 3000);

    let started = Instant::now();
    let refused = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-length", "3000")
                .body(Body::from(vec![b'z'; 3000]))
                .expect("request builds"),
        )
        .await;
    assert_eq!(refused.status, 503);
    assert_eq!(refused.header("retry-after"), Some("5"));
    assert_eq!(refused.header("connection"), Some("close"));
    assert!(started.elapsed() < Duration::from_secs(30));

    drop(sender);
    let held = holder.await.expect("holder joins");
    assert_eq!(held.status, 200);
    assert_eq!(harness.state.budget().in_use(), 0);

    let retried = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-length", "3000")
                .body(Body::from(vec![b'z'; 3000]))
                .expect("request builds"),
        )
        .await;
    assert_eq!(retried.status, 200);
    assert_eq!(harness.rows(&id).await.len(), 2);
}

#[tokio::test]
async fn slow_body_hits_the_read_timeout_and_releases_the_budget() {
    let harness = Harness::with_limits(CaptureLimits {
        body_read_timeout: Duration::from_millis(300),
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    let (sender, body) = channel_body();
    sender
        .send(Bytes::from_static(b"partial"))
        .await
        .expect("body reads");

    let started = Instant::now();
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .body(body)
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 408);
    assert_eq!(reply.header("connection"), Some("close"));
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(harness.rows(&id).await.is_empty());
    assert_eq!(harness.state.budget().in_use(), 0);
    drop(sender);
}

#[tokio::test]
async fn refuse_mode_stops_at_the_count_limit_and_stores_nothing_more() {
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 3,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    for _ in 0..3 {
        assert_eq!(harness.post(&format!("/{id}"), "abc").await.status, 200);
    }
    let refused = harness.post(&format!("/{id}"), "abc").await;
    assert_eq!(refused.status, 410);
    assert_eq!(
        refused.header("x-webhooks-live-error"),
        Some("endpoint_full")
    );
    assert_eq!(refused.header("x-endpoint-id"), Some(id.unpack()));
    assert_eq!(
        refused.body,
        "This endpoint holds the maximum of 3 captures. Delete captures to receive more."
    );
    assert_eq!(refused.header("connection"), Some("close"));

    let counters = harness.counters(&id).await;
    assert_eq!((counters.capture_count, counters.last_seq), (3, 3));
    assert_eq!(harness.rows(&id).await.len(), 3);
}

#[tokio::test]
async fn a_full_endpoint_refuses_before_any_body_byte_is_read() {
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 2,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    for _ in 0..2 {
        assert_eq!(harness.post(&format!("/{id}"), "abc").await.status, 200);
    }

    let polls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&polls);
    let silent = stream::poll_fn(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Poll::<Option<Result<Bytes, Infallible>>>::Pending
    });
    let refused = tokio::time::timeout(
        Duration::from_secs(5),
        harness.send(
            request("POST", &format!("/{id}"))
                .body(Body::from_stream(silent))
                .expect("request builds"),
        ),
    )
    .await
    .expect("the refusal does not wait for the body");

    assert_eq!(refused.status, 410);
    assert_eq!(
        refused.header("x-webhooks-live-error"),
        Some("endpoint_full")
    );
    assert_eq!(refused.header("x-endpoint-id"), Some(id.unpack()));
    assert_eq!(refused.header("connection"), Some("close"));
    assert_eq!(polls.load(Ordering::SeqCst), 0);
    assert_eq!(harness.state.budget().in_use(), 0);
    assert_eq!(harness.rows(&id).await.len(), 2);
}

#[tokio::test]
async fn a_full_ring_endpoint_still_reads_and_stores_the_body() {
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 2,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint_with("ring").await;
    for _ in 0..3 {
        assert_eq!(harness.post(&format!("/{id}"), "abc").await.status, 200);
    }
    assert_eq!(harness.counters(&id).await.capture_count, 2);
}

#[tokio::test]
async fn ring_mode_evicts_the_oldest_and_keeps_counters_exact() {
    let harness = Harness::with_limits(CaptureLimits {
        max_captures: 3,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint_with("ring").await;
    for _ in 0..5 {
        assert_eq!(harness.post(&format!("/{id}"), "abc").await.status, 200);
    }
    let rows = harness.rows(&id).await;
    let seqs: Vec<_> = rows.iter().map(|row| row.seq).collect();
    assert_eq!(seqs, [3, 4, 5]);
    let stored: i64 = rows.iter().map(|row| i64::from(row.stored_bytes)).sum();
    let counters = harness.counters(&id).await;
    assert_eq!(counters.capture_count, 3);
    assert_eq!(counters.last_seq, 5);
    assert_eq!(counters.bytes_used, stored);
}

#[tokio::test]
async fn storage_full_flag_answers_507_before_the_body_is_read() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    harness.state.storage_full().set(true);
    let reply = harness.post(&format!("/{id}"), "abc").await;
    assert_eq!(reply.status, 507);
    assert_eq!(reply.header("x-webhooks-live-error"), Some("storage_full"));
    assert_eq!(reply.header("connection"), Some("close"));
    harness.state.storage_full().set(false);
    assert_eq!(harness.post(&format!("/{id}"), "abc").await.status, 200);
}

#[tokio::test]
async fn closed_pool_answers_503_with_retry_after_and_error_header() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    harness.db.pool().close().await;
    let reply = harness.post(&format!("/{id}"), "abc").await;
    assert_eq!(reply.status, 503);
    assert_eq!(reply.header("retry-after"), Some("5"));
    assert_eq!(
        reply.header("x-webhooks-live-error"),
        Some("store_unavailable")
    );
    assert_eq!(reply.body, "Temporarily unavailable.");
}

#[tokio::test]
async fn hostile_multipart_with_many_parts_is_bounded_in_time_and_storage() {
    let harness = Harness::with_limits(CaptureLimits {
        max_body_bytes: 4 << 20,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    let part = b"--XX\r\nContent-Disposition: form-data; name=\"f\"\r\n\r\n\r\n";
    let mut body = Vec::with_capacity(4 << 20);
    while body.len() + part.len() < (4 << 20) - 16 {
        body.extend_from_slice(part);
    }
    body.extend_from_slice(b"--XX--\r\n");

    let started = Instant::now();
    let reply = harness
        .send(
            request("POST", &format!("/{id}"))
                .header("content-type", "multipart/form-data; boundary=XX")
                .body(Body::from(body))
                .expect("request builds"),
        )
        .await;
    assert_eq!(reply.status, 200);
    assert!(started.elapsed() < Duration::from_secs(30));
    let row = &harness.rows(&id).await[0];
    assert!(row.form.is_none());
    assert_eq!(row.body_size, row.body.len() as i32);
    assert_eq!(harness.state.budget().in_use(), 0);
}
