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
    let (live, title, owner): (bool, Option<String>, Option<whl_store::Uuid>) =
        sqlx::query_as("SELECT deleted_at IS NULL, title, owner_id FROM endpoints WHERE id = $1")
            .bind(endpoint)
            .fetch_one(harness.db.pool())
            .await
            .expect("the endpoint row exists");
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
