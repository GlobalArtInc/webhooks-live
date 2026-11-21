mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use common::tokens::mint;
use common::{Actor, Reply, SsoHarness, api_settings, mixed_harness, request, send};
use serde_json::{Value, json};
use whl_api::{ApiSettings, ApiState, NotifyApiSettings};
use whl_core::tier::{TierLimits, Tiers};
use whl_secrets::Keyring;
use whl_store::quota::UserSelector;

const KEYRING: &str = "1:a2tra2tra2tra2tra2tra2tra2tra2tra2tra2tra2s=";

fn app(harness: &SsoHarness, tiers: Tiers) -> Router {
    app_with(harness, tiers, api_settings())
}

fn app_with(harness: &SsoHarness, tiers: Tiers, settings: ApiSettings) -> Router {
    let keyring = Keyring::parse(KEYRING).expect("the key set is valid");
    whl_api::router(
        ApiState::new(harness.store.clone(), settings)
            .with_access(harness.access.clone())
            .with_notifications(NotifyApiSettings { enabled: true })
            .with_secrets(Arc::new(keyring))
            .with_tiers(tiers),
    )
}

async fn call(app: &Router, actor: Option<&Actor>, method: Method, uri: &str, body: &str) -> Reply {
    let mut built = request(method, uri, body);
    if let Some(actor) = actor {
        built.headers_mut().insert(
            header::COOKIE,
            actor.cookie.parse().expect("a valid cookie header"),
        );
    }
    send(app, built).await
}

fn with_member(change: impl FnOnce(&mut TierLimits)) -> Tiers {
    let mut member = TierLimits::member();
    change(&mut member);
    Tiers::default()
        .with_limits("member", member)
        .expect("the tier is valid")
}

fn item<'a>(usage: &'a Value, name: &str) -> &'a Value {
    usage["items"]
        .as_array()
        .expect("the items are an array")
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("the usage lists {name}"))
}

async fn create_endpoint(app: &Router, actor: &Actor) -> Reply {
    call(app, Some(actor), Method::POST, "/api/v1/endpoints", "{}").await
}

#[tokio::test]
async fn usage_reports_the_tier_and_the_counters_against_their_limits() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let created = harness
        .send_as(Some(&actor), Method::POST, "/api/v1/endpoints", "{}")
        .await;
    assert_eq!(created.status, StatusCode::CREATED);

    let reply = harness
        .send_as(Some(&actor), Method::GET, "/api/v1/usage", "")
        .await;

    assert_eq!(reply.status, StatusCode::OK);
    let usage = reply.json();
    assert_eq!(usage["tier"], "member");
    let endpoints = item(&usage, "endpoints");
    assert_eq!(endpoints["used"], 1);
    assert_eq!(endpoints["max"], 200);
    assert_eq!(endpoints["window"], "total");
    assert!(endpoints["resets_at"].is_null());
    let outbound = item(&usage, "outbound");
    assert_eq!(outbound["window"], "hour");
    assert_eq!(outbound["max"], 2000);
    assert!(outbound["resets_at"].is_string());
    assert_eq!(item(&usage, "storage_bytes")["max"], 2_147_483_648_u64);
}

#[tokio::test]
async fn usage_refuses_an_anonymous_caller() {
    let harness = mixed_harness().await;

    let reply = harness
        .send_as(None, Method::GET, "/api/v1/usage", "")
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.json()["error"]["code"], "sign_in_required");
}

#[tokio::test]
async fn the_instance_reports_the_tier_limits_and_channel_kinds_of_the_caller() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(
        &harness,
        with_member(|member| member.notification_kinds = vec!["webhook".to_owned()]),
    );

    let anonymous = call(&app, None, Method::GET, "/api/v1/instance", "")
        .await
        .json();
    let member = call(&app, Some(&actor), Method::GET, "/api/v1/instance", "")
        .await
        .json();

    assert_eq!(anonymous["tier"], "anonymous");
    assert_eq!(anonymous["limits"]["tokens"], 0);
    assert_eq!(anonymous["limits"]["rules_per_endpoint"], 5);
    assert_eq!(
        anonymous["features"]["notifications"],
        json!(["webhook", "slack", "discord", "telegram"])
    );
    assert_eq!(member["tier"], "member");
    assert_eq!(member["limits"]["tokens"], 20);
    assert_eq!(member["limits"]["max_captures_settable"], 10_000);
    assert_eq!(member["features"]["notifications"], json!(["webhook"]));
}

#[tokio::test]
async fn a_custom_tier_sets_the_token_limit_of_its_users() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let pro = TierLimits {
        tokens: 2,
        ..TierLimits::member()
    };
    let tiers = Tiers::default()
        .with_limits("pro", pro)
        .expect("the tier is valid");
    harness
        .store
        .set_user_tier(&UserSelector::Id(actor.id), "pro")
        .await
        .expect("the tier is set");
    let app = app(&harness, tiers);
    let body = json!({"name": "t", "scopes": ["read"], "expires_in_days": 30}).to_string();

    for _ in 0..2 {
        let created = call(&app, Some(&actor), Method::POST, "/api/v1/tokens", &body).await;
        assert_eq!(created.status, StatusCode::CREATED);
    }
    let refused = call(&app, Some(&actor), Method::POST, "/api/v1/tokens", &body).await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["error"]["code"], "limit_reached");
    assert_eq!(refused.json()["error"]["limit"]["name"], "tokens");
    assert_eq!(refused.json()["error"]["limit"]["max"], 2);
}

#[tokio::test]
async fn the_endpoint_count_of_the_tier_refuses_the_next_endpoint() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(&harness, with_member(|member| member.endpoints = 2));

    for _ in 0..2 {
        assert_eq!(
            create_endpoint(&app, &actor).await.status,
            StatusCode::CREATED
        );
    }
    let refused = create_endpoint(&app, &actor).await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["error"]["code"], "limit_reached");
    assert_eq!(refused.json()["error"]["limit"]["name"], "endpoints");
    assert_eq!(refused.json()["error"]["limit"]["max"], 2);
}

#[tokio::test]
async fn exports_are_counted_per_hour_and_refused_with_the_quota_shape() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(&harness, with_member(|member| member.exports_per_hour = 1));
    let id = create_endpoint(&app, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/export?format=json");

    let first = call(&app, Some(&actor), Method::GET, &uri, "").await;
    let second = call(&app, Some(&actor), Method::GET, &uri, "").await;

    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    let error = second.json();
    assert_eq!(error["error"]["code"], "quota_exceeded");
    assert_eq!(error["error"]["limit"]["name"], "exports");
    assert_eq!(error["error"]["limit"]["window"], "hour");
    assert_eq!(error["error"]["limit"]["max"], 1);
    assert!(error["error"]["limit"]["resets_at"].is_string());
    assert!(second.headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn never_expire_is_refused_with_quota_exceeded_while_over_the_storage_quota() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(
        &harness,
        with_member(|member| member.storage_bytes = Some(100)),
    );
    let id = create_endpoint(&app, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    sqlx::query("UPDATE endpoints SET bytes_used = 500 WHERE id = $1")
        .bind(&id)
        .execute(harness.db.pool())
        .await
        .expect("the usage is set");

    let refused = call(
        &app,
        Some(&actor),
        Method::PATCH,
        &format!("/api/v1/endpoints/{id}"),
        r#"{"retention":{"mode":"never"}}"#,
    )
    .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(!refused.headers.contains_key(header::RETRY_AFTER));
    assert_eq!(refused.json()["error"]["code"], "quota_exceeded");
    assert_eq!(refused.json()["error"]["limit"]["name"], "storage_bytes");
    assert_eq!(refused.json()["error"]["limit"]["window"], "total");
    assert_eq!(refused.json()["error"]["limit"]["max"], 100);
}

#[tokio::test]
async fn a_channel_kind_outside_the_tier_answers_tier_not_allowed() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(
        &harness,
        with_member(|member| member.notification_kinds = vec!["webhook".to_owned()]),
    );
    let id = create_endpoint(&app, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();

    let refused = call(
        &app,
        Some(&actor),
        Method::POST,
        &format!("/api/v1/endpoints/{id}/notifications"),
        r#"{"kind":"slack","name":"chat","config":{"url":"https://hooks.slack.com/services/T0/B0/token"}}"#,
    )
    .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "tier_not_allowed");
    assert_eq!(refused.json()["error"]["limit"]["name"], "slack");
}

#[tokio::test]
async fn the_rule_limit_of_an_owned_endpoint_follows_the_owner_tier() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(
        &harness,
        with_member(|member| member.rules_per_endpoint = 6),
    );
    let id = create_endpoint(&app, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/rules");

    for index in 0..6 {
        let body = json!({"name": format!("rule{index}"), "steps": [{"kind": "stop"}]});
        let created = call(&app, Some(&actor), Method::POST, &uri, &body.to_string()).await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.json());
    }
    let seventh = json!({"name": "seventh", "steps": [{"kind": "stop"}]});
    let refused = call(&app, Some(&actor), Method::POST, &uri, &seventh.to_string()).await;

    assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused.json()["error"]["code"], "limit_reached");
    assert_eq!(refused.json()["error"]["limit"]["name"], "rules");
    assert_eq!(refused.json()["error"]["limit"]["max"], 6);
}

#[tokio::test]
async fn the_anonymous_ceilings_never_exceed_the_instance_values() {
    let harness = mixed_harness().await;
    let strict = ApiSettings {
        retention: Duration::from_secs(86_400),
        max_captures: 100,
        ..api_settings()
    };
    let app = app_with(&harness, Tiers::default(), strict.clone());
    let created = call(&app, None, Method::POST, "/api/v1/endpoints", "{}").await;
    let id = created.json()["id"].as_str().expect("an id").to_owned();
    let uri = format!("/api/v1/endpoints/{id}");

    let captures = call(&app, None, Method::PATCH, &uri, r#"{"max_captures":500}"#).await;
    let retention = call(
        &app,
        None,
        Method::PATCH,
        &uri,
        r#"{"retention":{"mode":"sliding","seconds":604800}}"#,
    )
    .await;
    let instance = call(&app, None, Method::GET, "/api/v1/instance", "")
        .await
        .json();
    let lower = Tiers::default()
        .with_limits(
            "anonymous",
            TierLimits {
                max_captures: 50,
                ..TierLimits::anonymous()
            },
        )
        .expect("the tier is valid");
    let lowered = call(
        &app_with(&harness, lower, strict),
        None,
        Method::GET,
        "/api/v1/instance",
        "",
    )
    .await
    .json();

    assert_eq!(captures.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(retention.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(instance["limits"]["max_captures_settable"], 100);
    assert_eq!(instance["limits"]["max_retention_seconds"], 86_400);
    assert_eq!(lowered["limits"]["max_captures_settable"], 50);
}

#[tokio::test]
async fn after_a_downgrade_rules_can_be_removed_toggled_and_reordered_but_not_grown() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let roomy = app(
        &harness,
        with_member(|member| {
            member.rules_per_endpoint = 4;
            member.steps_per_rule = 3;
        }),
    );
    let id = create_endpoint(&roomy, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let uri = format!("/api/v1/endpoints/{id}/rules");
    let mut ids = Vec::new();
    for index in 0..3 {
        let steps = json!([{"kind": "stop"}, {"kind": "stop"}, {"kind": "stop"}]);
        let body = json!({"name": format!("rule{index}"), "steps": steps});
        let created = call(&roomy, Some(&actor), Method::POST, &uri, &body.to_string()).await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.json());
        ids.push(created.json()["id"].as_str().expect("an id").to_owned());
    }
    let tight = app(
        &harness,
        with_member(|member| {
            member.rules_per_endpoint = 2;
            member.steps_per_rule = 2;
        }),
    );

    let toggled = call(
        &tight,
        Some(&actor),
        Method::PATCH,
        &format!("{uri}/{}", ids[0]),
        r#"{"enabled":false}"#,
    )
    .await;
    let order = json!({"ids": [ids[2], ids[1], ids[0]]});
    let reordered = call(
        &tight,
        Some(&actor),
        Method::PUT,
        &format!("{uri}/order"),
        &order.to_string(),
    )
    .await;
    let removed = call(
        &tight,
        Some(&actor),
        Method::DELETE,
        &format!("{uri}/{}", ids[1]),
        "",
    )
    .await;
    let fourth = json!({"name": "fourth", "steps": [{"kind": "stop"}]});
    let grown = call(
        &tight,
        Some(&actor),
        Method::POST,
        &uri,
        &fourth.to_string(),
    )
    .await;
    let longer =
        json!({"steps": [{"kind": "stop"}, {"kind": "stop"}, {"kind": "stop"}, {"kind": "stop"}]});
    let lengthened = call(
        &tight,
        Some(&actor),
        Method::PATCH,
        &format!("{uri}/{}", ids[0]),
        &longer.to_string(),
    )
    .await;

    assert_eq!(toggled.status, StatusCode::OK, "{}", toggled.json());
    assert_eq!(reordered.status, StatusCode::OK, "{}", reordered.json());
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    assert_eq!(grown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(grown.json()["error"]["code"], "limit_reached");
    assert_eq!(lengthened.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(lengthened.json()["error"]["code"], "validation_failed");
}

#[tokio::test]
async fn usage_refuses_a_token_limited_to_endpoints_and_serves_an_unlimited_one() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let created = harness
        .send_as(Some(&actor), Method::POST, "/api/v1/endpoints", "{}")
        .await;
    let id = created.json()["id"].as_str().expect("an id").to_owned();
    let limited = mint(&harness, &actor, &["read"], Some(&[id.as_str()])).await;
    let unlimited = mint(&harness, &actor, &["read"], None).await;

    let refused = harness
        .send_bearer(&limited.token, Method::GET, "/api/v1/usage", "")
        .await;
    let served = harness
        .send_bearer(&unlimited.token, Method::GET, "/api/v1/usage", "")
        .await;

    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.json()["error"]["code"], "insufficient_scope");
    assert_eq!(served.status, StatusCode::OK);
}

#[tokio::test]
async fn an_export_refused_as_too_large_still_counts_against_the_hourly_quota() {
    let harness = mixed_harness().await;
    let actor = harness.actor("usr", "user@example.test", true).await;
    let app = app(
        &harness,
        with_member(|member| {
            member.exports_per_hour = 1;
            member.export_max_bytes = 10;
        }),
    );
    let id = create_endpoint(&app, &actor).await.json()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    sqlx::query(
        "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
         client_ip, headers, query, body_size, stored_bytes, response_status) \
         VALUES ($1, 1, gen_random_uuid(), now(), 'GET', 'http://x/', '', 'x', '127.0.0.1', \
         '[]', '[]', 0, 100, 200)",
    )
    .bind(&id)
    .execute(harness.db.pool())
    .await
    .expect("the capture inserts");
    let uri = format!("/api/v1/endpoints/{id}/export?format=json");

    let first = call(&app, Some(&actor), Method::GET, &uri, "").await;
    let second = call(&app, Some(&actor), Method::GET, &uri, "").await;

    assert_eq!(first.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(first.json()["error"]["code"], "export_too_large");
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.json()["error"]["code"], "quota_exceeded");
}
