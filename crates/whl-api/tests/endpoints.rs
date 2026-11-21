mod common;

use axum::http::{Method, StatusCode, header};
use common::{UNKNOWN_ID, bodiless, create, field_codes, harness, request, send};

#[tokio::test]
async fn create_without_body_uses_defaults() {
    let harness = harness().await;
    let reply = send(&harness.app, bodiless(Method::POST, "/api/v1/endpoints")).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    let body = reply.json();
    let id = body["id"].as_str().expect("id").to_owned();
    assert_eq!(id.len(), 24);
    assert_eq!(
        reply.headers.get(header::LOCATION).expect("location"),
        format!("/api/v1/endpoints/{id}").as_str()
    );
    assert_eq!(body["url"], format!("https://whl.test/{id}"));
    assert!(body["title"].is_null());
    assert_eq!(body["response"]["status"], 200);
    assert_eq!(body["response"]["content_type"], "text/plain");
    assert_eq!(body["response"]["headers"], serde_json::json!([]));
    assert_eq!(
        body["response"]["body"],
        serde_json::json!({"encoding": "utf8", "data": "", "truncated": false})
    );
    assert_eq!(body["response"]["delay_ms"], 0);
    assert_eq!(body["cors"], false);
    assert_eq!(body["overflow"], "refuse");
    assert_eq!(body["forward_rules"], serde_json::json!([]));
    assert_eq!(body["max_captures"], 500);
    assert_eq!(body["capture_count"], 0);
    assert_eq!(body["last_seq"], 0);
    assert!(body["owner"].is_null());
    assert!(body["my_role"].is_null());
}

#[tokio::test]
async fn create_with_custom_response() {
    let harness = harness().await;
    let body = create(
        &harness.app,
        r#"{"title":"stripe test","response":{"status":202,"content_type":"application/json",
            "headers":[{"name":"X-Test","value":"1"}],"body":{"encoding":"utf8","data":"{\"ok\":true}"},
            "delay_ms":500},"cors":true,"overflow":"ring"}"#,
    )
    .await;
    assert_eq!(body["title"], "stripe test");
    assert_eq!(body["response"]["status"], 202);
    assert_eq!(body["response"]["content_type"], "application/json");
    assert_eq!(
        body["response"]["headers"],
        serde_json::json!([{"name": "X-Test", "value": "1"}])
    );
    assert_eq!(body["response"]["body"]["data"], "{\"ok\":true}");
    assert_eq!(body["response"]["delay_ms"], 500);
    assert_eq!(body["cors"], true);
    assert_eq!(body["overflow"], "ring");
}

#[tokio::test]
async fn create_stores_a_base64_body_and_returns_it_as_base64() {
    let harness = harness().await;
    let body = create(
        &harness.app,
        r#"{"response":{"body":{"encoding":"base64","data":"/w=="}}}"#,
    )
    .await;
    assert_eq!(
        body["response"]["body"],
        serde_json::json!({"encoding": "base64", "data": "/w==", "truncated": false})
    );
}

#[tokio::test]
async fn create_returns_all_violations_in_one_response() {
    let harness = harness().await;
    let long_title = "t".repeat(101);
    let reply = send(
        &harness.app,
        request(
            Method::POST,
            "/api/v1/endpoints",
            &format!(
                r#"{{"title":"{long_title}","response":{{"status":99,"content_type":"not a type","delay_ms":20000}}}}"#
            ),
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = reply.json();
    assert_eq!(error["error"]["code"], "validation_failed");
    let codes = field_codes(&error);
    assert!(codes.contains(&("title".to_owned(), "too_long".to_owned())));
    assert!(codes.contains(&("response.status".to_owned(), "out_of_range".to_owned())));
    assert!(codes.contains(&(
        "response.content_type".to_owned(),
        "invalid_format".to_owned()
    )));
    assert!(codes.contains(&("response.delay_ms".to_owned(), "out_of_range".to_owned())));
    assert_eq!(codes.len(), 4);
}

#[tokio::test]
async fn get_returns_the_endpoint_and_extends_its_expiry() {
    let harness = harness().await;
    let created = create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '2 days' WHERE id = $1")
        .bind(&id)
        .execute(harness.db.pool())
        .await
        .expect("backdate the endpoint");

    let reply = send(
        &harness.app,
        bodiless(Method::GET, &format!("/api/v1/endpoints/{id}")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["id"], id.as_str());

    let recent: bool = sqlx::query_scalar(
        "SELECT last_active_at > now() - interval '1 day' FROM endpoints WHERE id = $1",
    )
    .bind(&id)
    .fetch_one(harness.db.pool())
    .await
    .expect("read last_active_at");
    assert!(recent);
}

#[tokio::test]
async fn get_of_an_unknown_id_is_gone() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        bodiless(Method::GET, &format!("/api/v1/endpoints/{UNKNOWN_ID}")),
    )
    .await;
    assert_eq!(reply.status, StatusCode::GONE);
    assert_eq!(reply.json()["error"]["code"], "endpoint_gone");
}

#[tokio::test]
async fn get_of_a_malformed_id_is_not_found() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        bodiless(Method::GET, "/api/v1/endpoints/short"),
    )
    .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["error"]["code"], "not_found");
}

#[tokio::test]
async fn patch_changes_fields_and_keeps_the_rest() {
    let harness = harness().await;
    let created = create(&harness.app, r#"{"title":"first"}"#).await;
    let id = created["id"].as_str().expect("id").to_owned();

    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"response":{"status":418},"cors":true}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let body = reply.json();
    assert_eq!(body["title"], "first");
    assert_eq!(body["response"]["status"], 418);
    assert_eq!(body["response"]["content_type"], "text/plain");
    assert_eq!(body["cors"], true);

    let reset = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"title":null}"#,
        ),
    )
    .await;
    assert!(reset.json()["title"].is_null());
}

#[tokio::test]
async fn patch_validates_the_merged_response() {
    let harness = harness().await;
    let created = create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{id}"),
            r#"{"response":{"status":99}}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("response.status".to_owned(), "out_of_range".to_owned())]
    );
}

#[tokio::test]
async fn patch_of_an_unknown_id_is_gone() {
    let harness = harness().await;
    let reply = send(
        &harness.app,
        request(
            Method::PATCH,
            &format!("/api/v1/endpoints/{UNKNOWN_ID}"),
            r#"{"cors":true}"#,
        ),
    )
    .await;
    assert_eq!(reply.status, StatusCode::GONE);
}

#[tokio::test]
async fn delete_removes_the_endpoint() {
    let harness = harness().await;
    let created = create(&harness.app, "").await;
    let id = created["id"].as_str().expect("id").to_owned();
    let uri = format!("/api/v1/endpoints/{id}");

    let deleted = send(&harness.app, bodiless(Method::DELETE, &uri)).await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);

    let after = send(&harness.app, bodiless(Method::GET, &uri)).await;
    assert_eq!(after.status, StatusCode::GONE);
    assert_eq!(after.json()["error"]["code"], "endpoint_gone");

    let again = send(&harness.app, bodiless(Method::DELETE, &uri)).await;
    assert_eq!(again.status, StatusCode::GONE);
}
