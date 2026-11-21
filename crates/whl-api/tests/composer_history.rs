mod common;

use axum::http::{Method, StatusCode};
use common::composer::{MockLocal, PEER, composer, error_code, limits, target};
use common::{Actor, Reply, SsoHarness, mixed_harness_with, sso_harness_with};
use serde_json::{Value, json};
use tokio::task::JoinSet;

const SELF_ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";
const ORIGIN: [(&str, &str); 1] = [("origin", "https://whl.test")];

fn self_url(path: &str) -> String {
    format!("https://whl.test/{SELF_ID}{path}")
}

async fn send_as(harness: &SsoHarness, actor: Option<&Actor>, body: &Value) -> Reply {
    harness
        .send_with(
            actor,
            Some(PEER),
            &ORIGIN,
            Method::POST,
            "/api/v1/send",
            &body.to_string(),
        )
        .await
}

async fn get_as(harness: &SsoHarness, actor: Option<&Actor>, uri: &str) -> Reply {
    harness
        .send_with(actor, Some(PEER), &ORIGIN, Method::GET, uri, "")
        .await
}

async fn delete_as(harness: &SsoHarness, actor: Option<&Actor>, uri: &str) -> Reply {
    harness
        .send_with(actor, Some(PEER), &ORIGIN, Method::DELETE, uri, "")
        .await
}

fn ids(reply: &Reply) -> Vec<String> {
    reply.json()["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["id"].as_str().expect("an id").to_owned())
        .collect()
}

#[tokio::test]
async fn history_keeps_the_newest_fifty_even_under_parallel_sends_and_pages_by_cursor() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 10_000))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;

    for wave in 0..15 {
        let mut tasks = JoinSet::new();
        for offset in 0..4 {
            let body =
                json!({"method": "GET", "url": self_url(&format!("/p{}", wave * 4 + offset))});
            let request =
                harness.request_as(&alice, Method::POST, "/api/v1/send", &body.to_string());
            let app = harness.app.clone();
            tasks.spawn(async move { common::send(&app, request).await });
        }
        while let Some(reply) = tasks.join_next().await {
            assert_eq!(reply.expect("the task finishes").status, StatusCode::OK);
        }
    }

    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM sent_requests WHERE user_id = $1")
        .bind(alice.id)
        .fetch_one(harness.db.pool())
        .await
        .expect("the rows are counted");
    assert_eq!(stored, 50);
    let all = get_as(&harness, Some(&alice), "/api/v1/send/history").await;
    assert_eq!(all.status, StatusCode::OK);
    let all_ids = ids(&all);
    assert_eq!(all_ids.len(), 50);
    assert!(all.json()["next_cursor"].is_null());

    let mut paged = Vec::new();
    let mut uri = "/api/v1/send/history?limit=20".to_owned();
    loop {
        let page = get_as(&harness, Some(&alice), &uri).await;
        paged.extend(ids(&page));
        match page.json()["next_cursor"].as_str() {
            Some(cursor) => uri = format!("/api/v1/send/history?limit=20&cursor={cursor}"),
            None => break,
        }
    }
    assert_eq!(paged, all_ids);
}

#[tokio::test]
async fn history_stores_the_request_with_credentials_redacted() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let body = json!({
        "method": "POST",
        "url": self_url("/hook?page=2&api_key=SECRET-QUERY"),
        "headers": [
            {"name": "Authorization", "value": "Bearer SECRET-HEADER"},
            {"name": "Cookie", "value": "sid=SECRET-COOKIE"},
            {"name": "Content-Type", "value": "application/json"},
            {"name": "X-Trace", "value": "visible"},
        ],
        "body": {"encoding": "utf8", "data": "{\"password\":\"SECRET-BODY\",\"n\":1}"},
    });
    assert_eq!(
        send_as(&harness, Some(&alice), &body).await.status,
        StatusCode::OK
    );

    let history = get_as(&harness, Some(&alice), "/api/v1/send/history").await;

    let text = String::from_utf8(history.body.clone()).expect("utf8");
    assert!(!text.contains("SECRET"), "{text}");
    assert!(text.contains("page=2"), "{text}");
    assert!(text.contains("visible"), "{text}");
    assert!(
        text.contains("\\\"n\\\":1") || text.contains("\"n\":1"),
        "{text}"
    );
    let stored: String = sqlx::query_scalar("SELECT request::text FROM sent_requests")
        .fetch_one(harness.db.pool())
        .await
        .expect("the row is read");
    assert!(!stored.contains("SECRET"), "{stored}");
}

#[tokio::test]
async fn a_body_that_cannot_be_redacted_is_not_stored() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let body = json!({
        "method": "POST",
        "url": self_url(""),
        "headers": [{"name": "Content-Type", "value": "text/plain"}],
        "body": {"encoding": "utf8", "data": "token=SECRET-PLAIN"},
    });
    assert_eq!(
        send_as(&harness, Some(&alice), &body).await.status,
        StatusCode::OK
    );

    let history = get_as(&harness, Some(&alice), "/api/v1/send/history").await;

    let item = &history.json()["items"][0];
    assert_eq!(item["request"]["body"], Value::Null);
    assert_eq!(item["request"]["body_size"], 18);
}

#[tokio::test]
async fn history_belongs_to_its_user_and_anonymous_callers_need_an_endpoint() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let bob = harness.actor("bob", "bob@example.test", true).await;
    let body = json!({"method": "GET", "url": self_url("")});
    let sent = send_as(&harness, Some(&alice), &body).await;
    let sent_id = sent.json()["id"].as_str().expect("an id").to_owned();

    let anonymous = get_as(&harness, None, "/api/v1/send/history").await;
    let bobs = get_as(&harness, Some(&bob), "/api/v1/send/history").await;
    let foreign_delete = delete_as(
        &harness,
        Some(&bob),
        &format!("/api/v1/send/history/{sent_id}"),
    )
    .await;
    let own_delete = delete_as(
        &harness,
        Some(&alice),
        &format!("/api/v1/send/history/{sent_id}"),
    )
    .await;
    let again = delete_as(
        &harness,
        Some(&alice),
        &format!("/api/v1/send/history/{sent_id}"),
    )
    .await;

    assert_eq!(anonymous.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&anonymous), "sign_in_required");
    assert!(ids(&bobs).is_empty());
    assert_eq!(foreign_delete.status, StatusCode::NOT_FOUND);
    assert_eq!(own_delete.status, StatusCode::NO_CONTENT);
    assert_eq!(again.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_anonymous_caller_keeps_history_per_endpoint_and_only_holders_can_read_it() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let open_endpoint = harness.anonymous_endpoint(None).await;
    let owned_endpoint = harness.endpoint_for(&alice).await;
    let body = json!({"method": "GET", "url": self_url(""), "endpoint_id": open_endpoint});
    let sent = send_as(&harness, None, &body).await;
    assert_eq!(sent.status, StatusCode::OK, "{}", sent.json());

    let listed = get_as(
        &harness,
        None,
        &format!("/api/v1/send/history?endpoint_id={open_endpoint}"),
    )
    .await;
    let foreign_send = send_as(
        &harness,
        None,
        &json!({"method": "GET", "url": self_url(""), "endpoint_id": owned_endpoint}),
    )
    .await;
    let foreign_list = get_as(
        &harness,
        None,
        &format!("/api/v1/send/history?endpoint_id={owned_endpoint}"),
    )
    .await;
    let entry = sent.json()["id"].as_str().expect("an id").to_owned();
    let removed = delete_as(&harness, None, &format!("/api/v1/send/history/{entry}")).await;

    assert_eq!(ids(&listed), [entry]);
    assert_eq!(foreign_send.status, StatusCode::GONE);
    assert_eq!(foreign_list.status, StatusCode::GONE);
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn in_sso_mode_the_composer_needs_a_session() {
    let local = MockLocal::new();
    let harness = sso_harness_with(composer(&local, limits(100, 100, 100))).await;
    let body = json!({"method": "GET", "url": self_url("")});

    let reply = send_as(&harness, None, &body).await;
    let history = get_as(&harness, None, "/api/v1/send/history").await;

    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(history.status, StatusCode::UNAUTHORIZED);
    assert!(local.delivered().is_empty());
}

#[tokio::test]
async fn a_signed_in_user_is_counted_per_user_not_per_network() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 1, 3))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let body = json!({"method": "GET", "url": self_url("")});

    for _ in 0..3 {
        assert_eq!(
            send_as(&harness, Some(&alice), &body).await.status,
            StatusCode::OK
        );
    }
    let refused = send_as(&harness, Some(&alice), &body).await;

    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.json()["error"]["limit"]["name"], "outbound");
    assert_eq!(refused.json()["error"]["limit"]["window"], "hour");
}

#[tokio::test]
async fn endpoint_history_stores_only_the_origin_of_a_target() {
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let body = json!({
        "method": "GET",
        "url": self_url("/hook?x=1"),
        "endpoint_id": endpoint,
    });
    assert_eq!(send_as(&harness, None, &body).await.status, StatusCode::OK);

    let listed = get_as(
        &harness,
        None,
        &format!("/api/v1/send/history?endpoint_id={endpoint}"),
    )
    .await;

    let item = &listed.json()["items"][0];
    assert_eq!(item["request"]["url"], "https://whl.test/\u{2026}");
    assert_eq!(item["final_url"], "https://whl.test/\u{2026}");
    assert!(!listed.json().to_string().contains(SELF_ID));
}

#[tokio::test]
async fn user_history_hides_token_like_path_segments_and_the_first_segment_of_an_own_url() {
    let remote = target().await;
    let local = MockLocal::new();
    let harness = mixed_harness_with(composer(&local, limits(100, 100, 100))).await;
    let alice = harness.actor("alice", "alice@example.test", true).await;
    let token_url = remote.url(&format!("/bot{}:{}/send", "1".repeat(9), "Z".repeat(30)));
    let first = send_as(
        &harness,
        Some(&alice),
        &json!({"method": "GET", "url": token_url}),
    )
    .await;
    let second = send_as(
        &harness,
        Some(&alice),
        &json!({"method": "GET", "url": self_url("/hook")}),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(second.status, StatusCode::OK);

    let history = get_as(&harness, Some(&alice), "/api/v1/send/history").await;

    let text = history.json().to_string();
    assert!(
        !text.contains(&"Z".repeat(30)) && !text.contains(SELF_ID),
        "{text}"
    );
    let urls: Vec<String> = history.json()["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["request"]["url"].as_str().expect("a url").to_owned())
        .collect();
    assert!(
        urls.contains(&"https://whl.test/[redacted]/hook".to_owned()),
        "{urls:?}"
    );
    assert!(
        urls.iter().any(|url| url.ends_with("/[redacted]/send")),
        "{urls:?}"
    );
}
