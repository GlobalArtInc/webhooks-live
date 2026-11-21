use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use axum::http::{Method, StatusCode};
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::forward::{CreateRuleOutcome, NewForwardRule};
use whl_store::{EndpointRole, GrantedRole, Store};

use super::{Actor, RETENTION, SsoHarness};

pub struct Cast {
    pub owner: Actor,
    pub editor: Actor,
    pub viewer: Actor,
    pub stranger: Actor,
}

pub async fn cast(harness: &SsoHarness) -> Cast {
    harness.actor("gue", "guest@example.test", true).await;
    Cast {
        owner: harness.actor("own", "owner@example.test", true).await,
        editor: harness.actor("edi", "editor@example.test", true).await,
        viewer: harness.actor("vie", "viewer@example.test", true).await,
        stranger: harness.actor("str", "stranger@example.test", true).await,
    }
}

pub struct Fixture {
    pub endpoint: String,
    pub capture: String,
    pub rule: String,
}

pub async fn store_capture(store: &Store, endpoint: &str) -> String {
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
                kind: whl_store::CaptureKind::Http,
                meta: serde_json::json!({}),
                fixed_status: None,
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

pub async fn fixture(harness: &SsoHarness, cast: &Cast, owned: bool) -> Fixture {
    let endpoint = if owned {
        harness.endpoint_for(&cast.owner).await
    } else {
        harness.anonymous_endpoint(None).await
    };
    let id = EndpointId::parse(&endpoint).expect("a valid endpoint id");
    if owned {
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
    let CreateRuleOutcome::Created(rule) = rule else {
        panic!("the rule was not created");
    };
    Fixture {
        endpoint,
        capture,
        rule: rule.id.to_string(),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
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

pub async fn snapshot(harness: &SsoHarness, endpoint: &str) -> Snapshot {
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

pub struct Route {
    pub method: Method,
    pub path: &'static str,
    pub body: &'static str,
    pub needed: EndpointRole,
    pub success: StatusCode,
    pub members: bool,
}

pub fn routes() -> Vec<Route> {
    use EndpointRole::{Editor, Owner, Viewer};
    let route = |method: Method, path, body, needed, success: u16| Route {
        method,
        path,
        body,
        needed,
        success: StatusCode::from_u16(success).expect("a valid status"),
        members: path.starts_with("/members"),
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
        route(Method::GET, "/captures", "", Viewer, 200),
        route(Method::GET, "/captures/latest", "", Viewer, 200),
        route(Method::GET, "/captures/{capture}", "", Viewer, 200),
        route(Method::GET, "/captures/{capture}/body", "", Viewer, 200),
        route(Method::GET, "/captures/{capture}/parts/0", "", Viewer, 404),
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

pub fn uri(route: &Route, fixture: &Fixture, cast: &Cast) -> String {
    let path = route
        .path
        .replace("{capture}", &fixture.capture)
        .replace("{rule}", &fixture.rule)
        .replace("{viewer}", &cast.viewer.id.hyphenated().to_string());
    format!("/api/v1/endpoints/{}{path}", fixture.endpoint)
}

pub fn error_code(reply: &super::Reply) -> String {
    reply.json()["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

const UNDECODABLE_IDS: [&str; 5] = ["%ff", "%c0%af", "%80", "ab%fe%ffcd", "%e2%28%a1"];

pub async fn assert_undecodable_ids_are_not_found(harness: &SsoHarness, actors: &[Option<&Actor>]) {
    let probes: [(Method, &str, &str, bool); 7] = [
        (Method::GET, "", "", true),
        (Method::GET, "/captures", "", true),
        (Method::PATCH, "", r#"{"title":"x"}"#, true),
        (Method::DELETE, "", "", true),
        (Method::GET, "/forward-rules", "", true),
        (Method::GET, "/members", "", false),
        (
            Method::PUT,
            "/members",
            r#"{"email":"a@b.test","role":"viewer"}"#,
            false,
        ),
    ];
    for id in UNDECODABLE_IDS {
        for (method, suffix, body, plain) in &probes {
            for actor in actors {
                let label = format!("{method} {id}{suffix}");
                let bad = harness
                    .send_as(
                        *actor,
                        method.clone(),
                        &format!("/api/v1/endpoints/{id}{suffix}"),
                        body,
                    )
                    .await;
                let malformed = harness
                    .send_as(
                        *actor,
                        method.clone(),
                        &format!("/api/v1/endpoints/not-an-id{suffix}"),
                        body,
                    )
                    .await;
                assert_ne!(bad.status, StatusCode::INTERNAL_SERVER_ERROR, "{label}");
                assert_eq!(bad.status, malformed.status, "{label}");
                assert_eq!(bad.body, malformed.body, "{label}");
                if *plain {
                    assert_eq!(bad.status, StatusCode::NOT_FOUND, "{label}");
                    assert_eq!(error_code(&bad), "not_found", "{label}");
                }
            }
        }
    }
}

pub async fn assert_undecodable_claim_is_not_found_once_signed_in(
    harness: &SsoHarness,
    signed_in: &Actor,
) {
    for id in UNDECODABLE_IDS {
        let uri = format!("/api/v1/endpoints/{id}/claim");
        let reply = harness
            .send_as(Some(signed_in), Method::POST, &uri, "")
            .await;
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{id}");
        assert_eq!(error_code(&reply), "not_found", "{id}");
        let anonymous = harness.send_as(None, Method::POST, &uri, "").await;
        assert_ne!(anonymous.status, StatusCode::INTERNAL_SERVER_ERROR, "{id}");
        assert_ne!(anonymous.status, StatusCode::NOT_FOUND, "{id}");
    }
}
