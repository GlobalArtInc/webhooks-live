mod common;

use std::sync::Arc;

use axum::Router;
use axum::http::{Method, StatusCode, header};
use common::matrix::error_code;
use common::tokens::{ALL_SCOPES, mint};
use common::{Actor, Reply, SsoHarness, api_settings, field_codes, mixed_harness, request, send};
use serde_json::{Value, json};
use whl_api::{ApiState, NotifyApiSettings, ScheduleSettings};
use whl_secrets::Keyring;

const KEYRING: &str = "1:a2tra2tra2tra2tra2tra2tra2tra2tra2tra2tra2s=";
const CHANNEL_URL: &str = "https://hooks.slack.com/services/T0/B0/fixture-channel-token";
const TARGET: &str = "https://8.8.8.8/hook";

struct Rig {
    harness: SsoHarness,
    app: Router,
}

async fn rig() -> Rig {
    let harness = mixed_harness().await;
    let state = ApiState::new(harness.store.clone(), api_settings())
        .with_access(harness.access.clone())
        .with_schedules(ScheduleSettings::enabled())
        .with_notifications(NotifyApiSettings { enabled: true })
        .with_secrets(Arc::new(Keyring::parse(KEYRING).expect("a valid key set")));
    let app = whl_api::router(state);
    Rig { harness, app }
}

impl Rig {
    async fn call(&self, actor: Option<&Actor>, method: Method, uri: &str, body: Value) -> Reply {
        let text = if body.is_null() {
            String::new()
        } else {
            body.to_string()
        };
        let mut built = request(method, uri, &text);
        if let Some(actor) = actor {
            built.headers_mut().insert(
                header::COOKIE,
                actor.cookie.parse().expect("a valid cookie header"),
            );
        }
        send(&self.app, built).await
    }

    async fn ok(&self, actor: &Actor, method: Method, uri: &str, body: Value) -> Value {
        let reply = self.call(Some(actor), method, uri, body).await;
        assert!(reply.status.is_success(), "{uri}: {:?}", reply.json());
        reply.json()
    }

    async fn owner(&self) -> Actor {
        self.harness
            .actor("owner", "owner@example.test", true)
            .await
    }

    async fn member(&self, owner: &Actor, endpoint: &str, tag: &str, role: &str) -> Actor {
        let email = format!("{tag}@example.test");
        let actor = self.harness.actor(tag, &email, true).await;
        self.ok(
            owner,
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            json!({ "email": email, "role": role }),
        )
        .await;
        actor
    }

    async fn source(&self, owner: &Actor) -> String {
        let endpoint = self.harness.endpoint_for(owner).await;
        let base = format!("/api/v1/endpoints/{endpoint}");
        self.ok(
            owner,
            Method::PATCH,
            &base,
            json!({
                "title": "Source",
                "tags": ["prod"],
                "cors": true,
                "response": {
                    "status": 202,
                    "body": {"encoding": "utf8", "data": "accepted"}
                }
            }),
        )
        .await;
        let forward = self
            .ok(
                owner,
                Method::POST,
                &format!("{base}/forward-rules"),
                json!({ "target_url": TARGET }),
            )
            .await;
        self.ok(
            owner,
            Method::POST,
            &format!("{base}/rules"),
            json!({
                "name": "relay",
                "steps": [{"kind": "forward", "forward_rule_id": forward["id"]}]
            }),
        )
        .await;
        self.ok(
            owner,
            Method::POST,
            &format!("{base}/schedules"),
            json!({
                "name": "probe",
                "cron": "*/5 * * * *",
                "request": {"method": "GET", "url": TARGET}
            }),
        )
        .await;
        self.ok(
            owner,
            Method::POST,
            &format!("{base}/notifications"),
            json!({"kind": "slack", "name": "alerts", "config": {"url": CHANNEL_URL}}),
        )
        .await;
        endpoint
    }

    async fn clone_of(&self, actor: &Actor, endpoint: &str, body: Value) -> Reply {
        self.call(
            Some(actor),
            Method::POST,
            &format!("/api/v1/endpoints/{endpoint}/clone"),
            body,
        )
        .await
    }

    async fn items(&self, actor: &Actor, endpoint: &str, part: &str) -> Vec<Value> {
        let reply = self
            .ok(
                actor,
                Method::GET,
                &format!("/api/v1/endpoints/{endpoint}/{part}"),
                Value::Null,
            )
            .await;
        reply["items"].as_array().cloned().unwrap_or_default()
    }
}

fn id_of(created: &Value) -> String {
    created["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned()
}

#[tokio::test]
async fn a_clone_copies_the_settings_and_parts_but_no_captures() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let reply = rig.clone_of(&owner, &source, json!({})).await;

    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    let created = reply.json();
    let clone = id_of(&created);
    assert_ne!(clone, source);
    assert_eq!(created["title"], "Source");
    assert_eq!(created["tags"], json!(["prod"]));
    assert_eq!(created["cors"], true);
    assert_eq!(created["response"]["status"], 202);
    assert_eq!(created["capture_count"], 0);
    assert_eq!(created["forward_rules"][0]["target_url"], TARGET);
    assert_eq!(rig.items(&owner, &clone, "rules").await.len(), 1);
    let schedules = rig.items(&owner, &clone, "schedules").await;
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0]["enabled"], false);
    let channels = rig.items(&owner, &clone, "notifications").await;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "alerts");
    assert!(!created.to_string().contains("fixture-channel-token"));
    assert!(!channels[0].to_string().contains("fixture-channel-token"));
}

#[tokio::test]
async fn rules_in_a_clone_point_at_the_cloned_forward_rules() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let created = rig.clone_of(&owner, &source, json!({})).await.json();

    let clone = id_of(&created);
    let rules = rig.items(&owner, &clone, "rules").await;
    let source_rules = rig.items(&owner, &source, "forward-rules").await;
    assert_eq!(
        rules[0]["steps"][0]["forward_rule_id"],
        created["forward_rules"][0]["id"]
    );
    assert_ne!(created["forward_rules"][0]["id"], source_rules[0]["id"]);
}

#[tokio::test]
async fn include_limits_the_copied_parts() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let created = rig
        .clone_of(
            &owner,
            &source,
            json!({"include": ["response"], "title": "Bare"}),
        )
        .await
        .json();

    let clone = id_of(&created);
    assert_eq!(created["title"], "Bare");
    assert_eq!(created["response"]["status"], 202);
    assert_eq!(created["forward_rules"], json!([]));
    assert!(rig.items(&owner, &clone, "rules").await.is_empty());
    assert!(rig.items(&owner, &clone, "schedules").await.is_empty());
    assert!(rig.items(&owner, &clone, "notifications").await.is_empty());
}

#[tokio::test]
async fn secret_values_are_never_copied_and_the_names_are_reported() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let base = format!("/api/v1/endpoints/{source}");
    rig.ok(
        &owner,
        Method::PUT,
        &format!("{base}/secrets/HOOKKEY"),
        json!({"value": "fixture-secret-value"}),
    )
    .await;
    rig.ok(
        &owner,
        Method::PATCH,
        &base,
        json!({"verifier": {"scheme": "github", "secret_name": "HOOKKEY"}}),
    )
    .await;

    let created = rig.clone_of(&owner, &source, json!({})).await.json();

    let clone = id_of(&created);
    assert_eq!(created["secrets_missing"], json!(["HOOKKEY"]));
    assert_eq!(created["verifier"], Value::Null);
    assert!(
        created["skipped"]
            .as_array()
            .is_some_and(|skipped| skipped.contains(&json!("verifier")))
    );
    assert!(rig.items(&owner, &clone, "secrets").await.is_empty());
}

#[tokio::test]
async fn asking_for_the_secrets_in_a_clone_is_refused() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let reply = rig
        .clone_of(&owner, &source, json!({"include": ["secrets"]}))
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("include.0".to_owned(), "not_allowed".to_owned())]
    );
}

#[tokio::test]
async fn a_caller_without_a_role_on_the_source_gets_gone() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let stranger = rig
        .harness
        .actor("stranger", "stranger@example.test", true)
        .await;

    let reply = rig.clone_of(&stranger, &source, json!({})).await;

    assert_eq!(reply.status, StatusCode::GONE);
}

#[tokio::test]
async fn a_viewer_clones_without_the_notification_channels() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let viewer = rig.member(&owner, &source, "viewer", "viewer").await;

    let reply = rig.clone_of(&viewer, &source, json!({})).await;

    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    let created = reply.json();
    let clone = id_of(&created);
    assert_eq!(
        created["owner"]["id"],
        json!(viewer.id.hyphenated().to_string())
    );
    assert_eq!(created["skipped"], json!(["channels"]));
    assert!(rig.items(&viewer, &clone, "notifications").await.is_empty());
    assert_eq!(rig.items(&viewer, &clone, "rules").await.len(), 1);
}

#[tokio::test]
async fn an_editor_clones_the_channels_with_their_urls_sealed_again() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let editor = rig.member(&owner, &source, "editor", "editor").await;

    let created = rig.clone_of(&editor, &source, json!({})).await.json();

    let clone = id_of(&created);
    let channels = rig.items(&editor, &clone, "notifications").await;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["kind"], "slack");
    assert!(!channels[0].to_string().contains("fixture-channel-token"));
}

#[tokio::test]
async fn a_clone_that_exceeds_the_schedule_limit_leaves_no_endpoint_behind() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    for _ in 0..4 {
        rig.clone_of(&owner, &source, json!({"include": ["schedules"]}))
            .await;
    }
    let listing = rig
        .ok(&owner, Method::GET, "/api/v1/endpoints", Value::Null)
        .await;
    let count = listing["items"].as_array().map_or(0, Vec::len);

    let reply = rig
        .clone_of(&owner, &source, json!({"include": ["schedules"]}))
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.json()["error"]["limit"]["name"], "schedules");
    let after = rig
        .ok(&owner, Method::GET, "/api/v1/endpoints", Value::Null)
        .await;
    assert_eq!(after["items"].as_array().map_or(0, Vec::len), count);
}

#[tokio::test]
async fn templates_are_listed_renamed_and_deleted_by_their_owner() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let created = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/templates",
            json!({"name": "starter", "from_endpoint_id": source}),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json());
    let template = created.json();
    let id = template["id"].as_str().expect("a template id").to_owned();
    assert_eq!(template["config"]["version"], 1);
    assert_eq!(template["config"]["endpoint"]["title"], "Source");
    assert_eq!(template["config"]["channels"], json!([]));
    assert!(!template.to_string().contains("fixture-channel-token"));

    let renamed = rig
        .ok(
            &owner,
            Method::PATCH,
            &format!("/api/v1/templates/{id}"),
            json!({"name": "renamed"}),
        )
        .await;
    assert_eq!(renamed["name"], "renamed");
    let listed = rig
        .ok(&owner, Method::GET, "/api/v1/templates", Value::Null)
        .await;
    assert_eq!(listed["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["items"][0]["name"], "renamed");
    assert!(listed["items"][0].get("config").is_none());
    let deleted = rig
        .call(
            Some(&owner),
            Method::DELETE,
            &format!("/api/v1/templates/{id}"),
            Value::Null,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let gone = rig
        .call(
            Some(&owner),
            Method::GET,
            &format!("/api/v1/templates/{id}"),
            Value::Null,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_template_name_is_unique_per_user() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let body = json!({"name": "starter", "from_endpoint_id": source});
    rig.ok(&owner, Method::POST, "/api/v1/templates", body.clone())
        .await;

    let reply = rig
        .call(Some(&owner), Method::POST, "/api/v1/templates", body)
        .await;

    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.json()["error"]["code"], "name_taken");
}

#[tokio::test]
async fn the_twenty_first_template_is_refused_with_the_limit() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.harness.endpoint_for(&owner).await;
    for index in 0..20 {
        rig.ok(
            &owner,
            Method::POST,
            "/api/v1/templates",
            json!({"name": format!("t{index}"), "from_endpoint_id": source}),
        )
        .await;
    }

    let reply = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/templates",
            json!({"name": "one-too-many", "from_endpoint_id": source}),
        )
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply.json()["error"]["limit"]["name"], "templates");
}

#[tokio::test]
async fn a_template_cannot_be_taken_from_an_endpoint_without_a_role() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let stranger = rig
        .harness
        .actor("stranger", "stranger@example.test", true)
        .await;

    let reply = rig
        .call(
            Some(&stranger),
            Method::POST,
            "/api/v1/templates",
            json!({"name": "stolen", "from_endpoint_id": source}),
        )
        .await;

    assert_eq!(reply.status, StatusCode::GONE);
}

#[tokio::test]
async fn templates_need_a_signed_in_caller() {
    let rig = rig().await;

    let reply = rig
        .call(None, Method::GET, "/api/v1/templates", Value::Null)
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(reply.json()["error"]["code"], "sign_in_required");
}

#[tokio::test]
async fn a_template_of_another_user_is_not_found() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let template = rig
        .ok(
            &owner,
            Method::POST,
            "/api/v1/templates",
            json!({"name": "private", "from_endpoint_id": source}),
        )
        .await;
    let id = template["id"].as_str().expect("a template id").to_owned();
    let other = rig.harness.actor("other", "other@example.test", true).await;

    let shown = rig
        .call(
            Some(&other),
            Method::GET,
            &format!("/api/v1/templates/{id}"),
            Value::Null,
        )
        .await;
    let used = rig
        .call(
            Some(&other),
            Method::POST,
            "/api/v1/endpoints",
            json!({"template_id": id}),
        )
        .await;

    assert_eq!(shown.status, StatusCode::NOT_FOUND);
    assert_eq!(used.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_endpoint_created_from_a_template_gets_its_parts_and_the_overrides() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let template = rig
        .ok(
            &owner,
            Method::POST,
            "/api/v1/templates",
            json!({"name": "starter", "from_endpoint_id": source}),
        )
        .await;

    let reply = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/endpoints",
            json!({"template_id": template["id"], "title": "From template"}),
        )
        .await;

    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    let created = reply.json();
    let clone = id_of(&created);
    assert_eq!(created["title"], "From template");
    assert_eq!(created["response"]["status"], 202);
    assert_eq!(created["forward_rules"][0]["target_url"], TARGET);
    assert_eq!(rig.items(&owner, &clone, "rules").await.len(), 1);
    assert_eq!(rig.items(&owner, &clone, "schedules").await.len(), 1);
    assert!(rig.items(&owner, &clone, "notifications").await.is_empty());
}

#[tokio::test]
async fn a_tampered_template_is_validated_again_on_create() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let template = rig
        .ok(
            &owner,
            Method::POST,
            "/api/v1/templates",
            json!({"name": "starter", "from_endpoint_id": source}),
        )
        .await;
    sqlx::query(
        "UPDATE endpoint_templates SET config = jsonb_set(config, '{forward_rules,0,target_url}', \
         '\"http://127.0.0.1/admin\"')",
    )
    .execute(rig.harness.db.pool())
    .await
    .expect("the template is tampered with");

    let reply = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/endpoints",
            json!({"template_id": template["id"]}),
        )
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        field_codes(&reply.json())
            .iter()
            .any(|(field, _)| field == "forward_rules.0.target_url")
    );
}

#[tokio::test]
async fn an_oversized_template_document_is_refused() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let template = rig
        .ok(
            &owner,
            Method::POST,
            "/api/v1/templates",
            json!({"name": "starter", "from_endpoint_id": source}),
        )
        .await;
    sqlx::query(
        "UPDATE endpoint_templates SET config = jsonb_set(config, '{endpoint,title}', \
         to_jsonb(repeat('x', 600000)))",
    )
    .execute(rig.harness.db.pool())
    .await
    .expect("the template is padded");

    let reply = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/endpoints",
            json!({"template_id": template["id"]}),
        )
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("config".to_owned(), "too_long".to_owned())]
    );
}

#[tokio::test]
async fn api_tokens_cannot_clone_or_manage_templates() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.harness.endpoint_for(&owner).await;
    let minted = mint(&rig.harness, &owner, &ALL_SCOPES, None).await;

    for (method, uri) in [
        (Method::POST, format!("/api/v1/endpoints/{source}/clone")),
        (Method::GET, "/api/v1/templates".to_owned()),
    ] {
        let reply = rig
            .harness
            .send_bearer(&minted.token, method, &uri, "")
            .await;

        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(error_code(&reply), "session_required", "{uri}");
    }
}

async fn notify_rule(rig: &Rig, owner: &Actor, source: &str) -> String {
    let channels = rig.items(owner, source, "notifications").await;
    let channel = channels[0]["id"].as_str().expect("a channel id").to_owned();
    rig.ok(
        owner,
        Method::POST,
        &format!("/api/v1/endpoints/{source}/rules"),
        json!({"name": "alert", "steps": [{"kind": "notify", "channel_id": channel}]}),
    )
    .await;
    channel
}

#[tokio::test]
async fn a_notify_step_in_a_clone_points_at_the_cloned_channel() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    let source_channel = notify_rule(&rig, &owner, &source).await;

    let created = rig.clone_of(&owner, &source, json!({})).await.json();

    let clone = id_of(&created);
    let channels = rig.items(&owner, &clone, "notifications").await;
    let rules = rig.items(&owner, &clone, "rules").await;
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[1]["steps"][0]["channel_id"], channels[0]["id"]);
    assert_ne!(channels[0]["id"], json!(source_channel));
}

#[tokio::test]
async fn a_rule_notifying_a_channel_that_was_not_copied_is_skipped() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;
    notify_rule(&rig, &owner, &source).await;
    let viewer = rig.member(&owner, &source, "viewer", "viewer").await;

    let created = rig.clone_of(&viewer, &source, json!({})).await.json();

    let clone = id_of(&created);
    assert_eq!(rig.items(&viewer, &clone, "rules").await.len(), 1);
    let skipped = created["skipped"].as_array().cloned().unwrap_or_default();
    assert!(skipped.contains(&json!("rules.1")));
    assert!(skipped.contains(&json!("channels")));
}

#[tokio::test]
async fn rules_that_forward_need_the_forward_rules_in_include() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let reply = rig
        .clone_of(&owner, &source, json!({"include": ["rules"]}))
        .await;

    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        field_codes(&reply.json()),
        vec![("include".to_owned(), "not_allowed".to_owned())]
    );
}

#[tokio::test]
async fn a_token_cannot_create_an_endpoint_from_a_template() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let minted = mint(&rig.harness, &owner, &["endpoints:write"], None).await;

    let reply = rig
        .harness
        .send_bearer(
            &minted.token,
            Method::POST,
            "/api/v1/endpoints",
            r#"{"template_id":"0190c3a2-7d1e-7000-8000-000000000001"}"#,
        )
        .await;

    assert_eq!(reply.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&reply), "session_required");
}

#[tokio::test]
async fn parallel_clones_with_schedules_both_succeed() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.source(&owner).await;

    let (first, second) = tokio::join!(
        rig.clone_of(&owner, &source, json!({})),
        rig.clone_of(&owner, &source, json!({}))
    );

    assert_eq!(first.status, StatusCode::CREATED, "{:?}", first.json());
    assert_eq!(second.status, StatusCode::CREATED, "{:?}", second.json());
}

#[tokio::test]
async fn a_response_body_with_a_nul_byte_survives_a_template() {
    let rig = rig().await;
    let owner = rig.owner().await;
    let source = rig.harness.endpoint_for(&owner).await;
    rig.ok(
        &owner,
        Method::PATCH,
        &format!("/api/v1/endpoints/{source}"),
        json!({"response": {"body": {"encoding": "base64", "data": "YQBi"}}}),
    )
    .await;
    let template = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/templates",
            json!({"name": "binary", "from_endpoint_id": source}),
        )
        .await;
    assert_eq!(
        template.status,
        StatusCode::CREATED,
        "{:?}",
        template.json()
    );

    let created = rig
        .call(
            Some(&owner),
            Method::POST,
            "/api/v1/endpoints",
            json!({"template_id": template.json()["id"]}),
        )
        .await;

    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.json());
    assert_eq!(created.json()["response"]["body"]["data"], "a\u{0}b");
}
