mod common;

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::{Actor, RETENTION, SsoHarness, UNKNOWN_ID, sso_harness};
use serde_json::{Value, json};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::forward::NewForwardRule;
use whl_store::{EndpointRole, GrantedRole, Store};

struct Cast {
    owner: Actor,
    editor: Actor,
    viewer: Actor,
    other: Actor,
}

async fn cast(harness: &SsoHarness) -> Cast {
    harness.actor("gue", "guest@example.test", true).await;
    Cast {
        owner: harness.actor("own", "owner@example.test", true).await,
        editor: harness.actor("edi", "editor@example.test", true).await,
        viewer: harness.actor("vie", "viewer@example.test", true).await,
        other: harness.actor("oth", "other@example.test", true).await,
    }
}

struct Fixture {
    endpoint: String,
    capture: String,
    rule: String,
}

async fn store_capture(store: &Store, endpoint: &str) -> String {
    let id = CaptureId::generate().expect("an id is generated");
    let outcome = store
        .store_capture(
            NewCapture {
                id,
                endpoint_id: EndpointId::parse(endpoint).expect("a valid endpoint id"),
                received_at: OffsetDateTime::now_utc(),
                method: "POST".to_owned(),
                url: "http://h/x".to_owned(),
                path: "/x".to_owned(),
                query_string: String::new(),
                host: "h".to_owned(),
                client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                headers: Vec::new(),
                query: Vec::new(),
                form: None,
                content_type: None,
                body: Default::default(),
                status_override: None,
            },
            CapturePolicy {
                max_captures: 500,
                retention: Duration::from_secs(3600),
            },
        )
        .await
        .expect("the capture is stored");
    assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
    id.to_string()
}

async fn fixture(harness: &SsoHarness, cast: &Cast) -> Fixture {
    let endpoint = harness.endpoint_for(&cast.owner).await;
    let id = EndpointId::parse(&endpoint).expect("a valid endpoint id");
    for (actor, role) in [
        (&cast.editor, GrantedRole::Editor),
        (&cast.viewer, GrantedRole::Viewer),
    ] {
        harness
            .store
            .set_member(&id, RETENTION, &actor.email, role)
            .await
            .expect("the member is stored");
    }
    let capture = store_capture(&harness.store, &endpoint).await;
    let rule = harness
        .store
        .create_forward_rule(
            &id,
            RETENTION,
            &NewForwardRule {
                enabled: true,
                target_url: "http://8.8.8.8/hook".to_owned(),
                method: None,
                content_type: None,
                header_allowlist: Vec::new(),
                append_path: false,
            },
        )
        .await
        .expect("the rule is stored");
    let whl_store::forward::CreateRuleOutcome::Created(rule) = rule else {
        panic!("the rule was not created");
    };
    Fixture {
        endpoint,
        capture,
        rule: rule.id.to_string(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    captures: i64,
    rules: i64,
    members: i64,
    live: bool,
    title: Option<String>,
    enabled_rules: i64,
    owner: Option<whl_store::Uuid>,
    last_active_at: OffsetDateTime,
    forward_jobs: i64,
}

async fn snapshot(harness: &SsoHarness, endpoint: &str) -> Snapshot {
    let count = |table: &'static str| {
        let pool = harness.db.pool().clone();
        let endpoint = endpoint.to_owned();
        async move {
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT count(*) FROM {table} WHERE endpoint_id = $1"
            ))
            .bind(endpoint)
            .fetch_one(&pool)
            .await
            .expect("the count runs")
        }
    };
    let (live, title, owner, last_active_at): (
        bool,
        Option<String>,
        Option<whl_store::Uuid>,
        OffsetDateTime,
    ) = sqlx::query_as(
        "SELECT deleted_at IS NULL, title, owner_id, last_active_at FROM endpoints WHERE id = $1",
    )
    .bind(endpoint)
    .fetch_one(harness.db.pool())
    .await
    .expect("the endpoint row exists");
    let forward_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM forward_jobs j JOIN captures c ON c.id = j.capture_id \
         WHERE c.endpoint_id = $1",
    )
    .bind(endpoint)
    .fetch_one(harness.db.pool())
    .await
    .expect("the count runs");
    let enabled_rules: i64 =
        sqlx::query_scalar("SELECT count(*) FROM forward_rules WHERE endpoint_id = $1 AND enabled")
            .bind(endpoint)
            .fetch_one(harness.db.pool())
            .await
            .expect("the count runs");
    Snapshot {
        captures: count("captures").await,
        rules: count("forward_rules").await,
        members: count("endpoint_members").await,
        live,
        title,
        enabled_rules,
        owner,
        last_active_at,
        forward_jobs,
    }
}

struct Route {
    method: Method,
    path: &'static str,
    body: &'static str,
    needed: EndpointRole,
    success: StatusCode,
}

fn routes() -> Vec<Route> {
    use EndpointRole::{Editor, Owner, Viewer};
    let route = |method: Method, path, body, needed, success: u16| Route {
        method,
        path,
        body,
        needed,
        success: StatusCode::from_u16(success).expect("a valid status"),
    };
    vec![
        route(Method::GET, "", "", Viewer, 200),
        route(Method::PATCH, "", r#"{"title":"renamed"}"#, Editor, 200),
        route(Method::DELETE, "", "", Owner, 204),
        route(Method::GET, "/members", "", Owner, 200),
        route(
            Method::PUT,
            "/members",
            r#"{"email":"guest@example.test","role":"viewer"}"#,
            Owner,
            200,
        ),
        route(Method::DELETE, "/members/{viewer}", "", Owner, 204),
        route(Method::GET, "/forward-rules", "", Viewer, 200),
        route(
            Method::POST,
            "/forward-rules",
            r#"{"target_url":"http://8.8.8.8/other"}"#,
            Editor,
            201,
        ),
        route(
            Method::PATCH,
            "/forward-rules/{rule}",
            r#"{"enabled":false}"#,
            Editor,
            200,
        ),
        route(Method::DELETE, "/forward-rules/{rule}", "", Editor, 204),
        route(Method::GET, "/rules", "", Viewer, 200),
        route(
            Method::POST,
            "/rules",
            r#"{"name":"answer","steps":[{"kind":"stop"}]}"#,
            Editor,
            201,
        ),
        route(Method::GET, "/captures", "", Viewer, 200),
        route(Method::GET, "/captures/latest", "", Viewer, 200),
        route(Method::GET, "/captures/{capture}", "", Viewer, 200),
        route(Method::GET, "/captures/{capture}/body", "", Viewer, 200),
        route(Method::DELETE, "/captures/{capture}", "", Editor, 204),
        route(Method::DELETE, "/captures", "", Editor, 204),
        route(
            Method::POST,
            "/captures/{capture}/forward",
            r#"{"target_url":"http://8.8.8.8/x"}"#,
            Editor,
            202,
        ),
        route(
            Method::POST,
            "/captures/{capture}/resend",
            r#"{"target_url":"http://8.8.8.8/x"}"#,
            Editor,
            202,
        ),
    ]
}

fn uri(route: &Route, fixture: &Fixture, cast: &Cast) -> String {
    let path = route
        .path
        .replace("{capture}", &fixture.capture)
        .replace("{rule}", &fixture.rule)
        .replace("{viewer}", &cast.viewer.id.hyphenated().to_string());
    format!("/api/v1/endpoints/{}{path}", fixture.endpoint)
}

fn error_code(reply: &common::Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn the_role_matrix_holds_on_every_endpoint_route() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let callers: [(&str, Option<&Actor>, Option<EndpointRole>); 5] = [
        ("anonymous", None, None),
        ("other user", Some(&cast.other), None),
        ("viewer", Some(&cast.viewer), Some(EndpointRole::Viewer)),
        ("editor", Some(&cast.editor), Some(EndpointRole::Editor)),
        ("owner", Some(&cast.owner), Some(EndpointRole::Owner)),
    ];
    for route in routes() {
        for (name, actor, role) in callers {
            let fixture = fixture(&harness, &cast).await;
            let before = snapshot(&harness, &fixture.endpoint).await;
            let target = uri(&route, &fixture, &cast);
            let reply = harness
                .send_as(actor, route.method.clone(), &target, route.body)
                .await;
            let label = format!("{} {} as {name}", route.method, route.path);
            match role {
                None => {
                    assert_eq!(reply.status, StatusCode::GONE, "{label}");
                    assert_eq!(error_code(&reply), "endpoint_gone", "{label}");
                }
                Some(held) if held < route.needed => {
                    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{label}");
                    assert_eq!(error_code(&reply), "forbidden", "{label}");
                }
                Some(_) => {
                    assert_eq!(reply.status, route.success, "{label}");
                    continue;
                }
            }
            assert_eq!(
                snapshot(&harness, &fixture.endpoint).await,
                before,
                "{label} left the data untouched"
            );
        }
    }
}

#[tokio::test]
async fn a_caller_without_a_role_cannot_tell_a_real_endpoint_from_an_unknown_one() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast).await;
    for actor in [None, Some(&cast.other)] {
        for (method, suffix, body) in [
            (Method::GET, "", ""),
            (Method::PATCH, "", "not json at all"),
            (Method::DELETE, "/captures", ""),
            (Method::PUT, "/members", r#"{"role":"owner"}"#),
        ] {
            let real = harness
                .send_as(
                    actor,
                    method.clone(),
                    &format!("/api/v1/endpoints/{}{suffix}", fixture.endpoint),
                    body,
                )
                .await;
            let unknown = harness
                .send_as(
                    actor,
                    method.clone(),
                    &format!("/api/v1/endpoints/{UNKNOWN_ID}{suffix}"),
                    body,
                )
                .await;
            assert_eq!(real.status, StatusCode::GONE, "{method} {suffix}");
            assert_eq!(real.status, unknown.status);
            assert_eq!(real.body, unknown.body);
        }
    }
}

#[tokio::test]
async fn the_role_check_runs_before_the_body_is_validated() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast).await;
    let target = format!("/api/v1/endpoints/{}", fixture.endpoint);
    let invalid = r#"{"response":{"status":9999}}"#;

    let viewer = harness
        .send_as(Some(&cast.viewer), Method::PATCH, &target, invalid)
        .await;
    assert_eq!(viewer.status, StatusCode::FORBIDDEN);
    let editor = harness
        .send_as(Some(&cast.editor), Method::PATCH, &target, invalid)
        .await;
    assert_eq!(editor.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn an_expired_or_deleted_endpoint_is_gone_even_for_its_owner() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let expired = harness.endpoint_for(&cast.owner).await;
    sqlx::query("UPDATE endpoints SET last_active_at = now() - interval '30 days' WHERE id = $1")
        .bind(&expired)
        .execute(harness.db.pool())
        .await
        .expect("the endpoint can be aged");
    let deleted = harness.endpoint_for(&cast.owner).await;
    let removed = harness
        .send_as(
            Some(&cast.owner),
            Method::DELETE,
            &format!("/api/v1/endpoints/{deleted}"),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    for id in [expired, deleted] {
        let reply = harness
            .send_as(
                Some(&cast.owner),
                Method::GET,
                &format!("/api/v1/endpoints/{id}"),
                "",
            )
            .await;
        assert_eq!(reply.status, StatusCode::GONE);
    }
}

#[tokio::test]
async fn a_malformed_endpoint_id_is_not_found_for_everyone() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    for actor in [None, Some(&cast.owner)] {
        let reply = harness
            .send_as(actor, Method::GET, "/api/v1/endpoints/not-an-id", "")
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND);
        assert_eq!(error_code(&reply), "not_found");
    }
}

#[tokio::test]
async fn an_undecodable_endpoint_id_is_not_found_like_a_malformed_one_for_every_caller() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let actors = [
        None,
        Some(&cast.other),
        Some(&cast.viewer),
        Some(&cast.editor),
        Some(&cast.owner),
    ];
    common::matrix::assert_undecodable_ids_are_not_found(&harness, &actors).await;
    common::matrix::assert_undecodable_claim_is_not_found_once_signed_in(&harness, &cast.other)
        .await;
}

async fn get_with_cookies(harness: &SsoHarness, uri: &str, cookies: &[&str]) -> common::Reply {
    let mut builder = axum::http::Request::builder().uri(uri);
    for cookie in cookies {
        builder = builder.header(axum::http::header::COOKIE, *cookie);
    }
    let request = builder
        .body(axum::body::Body::empty())
        .expect("a valid request");
    common::send(&harness.app, request).await
}

#[tokio::test]
async fn smuggled_cookie_names_neither_shadow_nor_grant_a_session() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let endpoint = harness.endpoint_for(&cast.owner).await;
    let target = format!("/api/v1/endpoints/{endpoint}");
    let real = cast.owner.cookie.clone();
    let editor_token = cast.editor.cookie.split_once('=').expect("a pair").1;
    let owner_token = real.split_once('=').expect("a pair").1;

    let doubled = format!("{real}; {real}");
    let encoded_twin = format!("{real}; %5F%5FHost-whl_session={editor_token}");
    let encoded_first = format!("%5F%5FHost-whl_session={editor_token}; {real}");
    let encoded_alone = format!("%5F%5FHost-whl_session={owner_token}");
    let lowercase = format!("__host-whl_session={owner_token}");
    let spaced = format!("__Host-whl_session ={owner_token}");

    assert_eq!(
        get_with_cookies(&harness, &target, &[&doubled])
            .await
            .status,
        StatusCode::GONE
    );
    assert_eq!(
        get_with_cookies(&harness, &target, &[&real, &real])
            .await
            .status,
        StatusCode::GONE
    );
    for cookie in [&encoded_twin, &encoded_first] {
        let reply = get_with_cookies(&harness, &target, &[cookie]).await;
        assert_eq!(reply.status, StatusCode::OK, "{cookie}");
        assert_eq!(reply.json()["my_role"], "owner", "{cookie}");
    }
    for cookie in [&encoded_alone, &lowercase, &spaced] {
        let reply = get_with_cookies(&harness, &target, &[cookie]).await;
        assert_eq!(reply.status, StatusCode::GONE, "{cookie}");
    }
}

#[tokio::test]
async fn creating_an_endpoint_needs_a_session_and_makes_the_creator_the_owner() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;

    let anonymous = harness
        .send_as(None, Method::POST, "/api/v1/endpoints", "")
        .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&anonymous), "unauthenticated");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoints")
        .fetch_one(harness.db.pool())
        .await
        .expect("the count runs");
    assert_eq!(count, 0);

    let created = harness
        .send_as(
            Some(&cast.owner),
            Method::POST,
            "/api/v1/endpoints",
            r#"{"title":"mine"}"#,
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED);
    let body = created.json();
    assert_eq!(body["my_role"], "owner");
    assert_eq!(body["owner"]["email"], "owner@example.test");
    assert_eq!(
        body["owner"]["id"],
        Value::String(cast.owner.id.hyphenated().to_string())
    );
    let stored: Option<whl_store::Uuid> =
        sqlx::query_scalar("SELECT owner_id FROM endpoints WHERE id = $1")
            .bind(body["id"].as_str().expect("an id"))
            .fetch_one(harness.db.pool())
            .await
            .expect("the endpoint row exists");
    assert_eq!(stored, Some(cast.owner.id));
}

#[tokio::test]
async fn the_endpoint_object_reports_the_callers_role_and_the_owner() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let fixture = fixture(&harness, &cast).await;
    let target = format!("/api/v1/endpoints/{}", fixture.endpoint);
    for (actor, role) in [
        (&cast.viewer, "viewer"),
        (&cast.editor, "editor"),
        (&cast.owner, "owner"),
    ] {
        let reply = harness.send_as(Some(actor), Method::GET, &target, "").await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.json()["my_role"], role);
        assert_eq!(reply.json()["owner"]["email"], "owner@example.test");
    }
}

#[tokio::test]
async fn the_instance_route_reports_the_signed_in_user_and_never_answers_401() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;

    let anonymous = harness
        .send_as(None, Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(anonymous.status, StatusCode::OK);
    assert_eq!(anonymous.json()["auth_mode"], "sso");
    assert_eq!(anonymous.json()["user"], Value::Null);
    assert_eq!(anonymous.json()["auth"]["provider_name"], "Test SSO");

    let signed_in = harness
        .send_as(Some(&cast.editor), Method::GET, "/api/v1/instance", "")
        .await;
    assert_eq!(
        signed_in.json()["user"],
        json!({
            "id": cast.editor.id.hyphenated().to_string(),
            "email": "editor@example.test",
            "name": "User edi",
        })
    );
}
