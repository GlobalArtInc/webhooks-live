mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, RawPathParams, Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use axum::routing::get;
use common::{Harness, Jar, send};
use serde_json::{Value, json};
use whl_auth::{Access, AccessControl, AccessMode, Caller, EndpointParam, Requirement};
use whl_core::client_ip::TrustedProxies;
use whl_store::{EndpointRole, EndpointSettings, Overflow};

const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
struct Gate {
    access: AccessControl,
    requirement: Requirement,
}

async fn gate(
    State(gate): State<Gate>,
    params: Result<RawPathParams, axum::extract::rejection::RawPathParamsRejection>,
    request: Request,
    next: Next,
) -> Response {
    let id = EndpointParam::from_path(&params, "id");
    gate.access
        .enforce(gate.requirement, id, request, next)
        .await
}

async fn who(caller: Caller) -> axum::Json<Value> {
    axum::Json(json!({
        "email": caller.user().map(|user| user.email.clone()),
        "ip": caller.ip().map(|ip| ip.to_string()),
        "network": caller.network().map(|net| net.to_string()),
    }))
}

async fn role(access: Access) -> axum::Json<Value> {
    axum::Json(json!({
        "role": access.role().map(EndpointRole::as_str),
        "signed_in": access.caller().user().is_some(),
    }))
}

fn probes(access: &AccessControl) -> Router {
    let guarded = |requirement: Requirement| Gate {
        access: access.clone(),
        requirement,
    };
    Router::new()
        .merge(
            Router::new()
                .route("/probe/anyone", get(who))
                .route_layer(from_fn_with_state(guarded(Requirement::Anyone), gate)),
        )
        .merge(
            Router::new()
                .route("/probe/signed-in", get(who))
                .route_layer(from_fn_with_state(guarded(Requirement::SignedIn), gate)),
        )
        .merge(
            Router::new()
                .route("/probe/viewer/{id}", get(role))
                .route_layer(from_fn_with_state(
                    guarded(Requirement::Role(EndpointRole::Viewer)),
                    gate,
                )),
        )
        .merge(
            Router::new()
                .route("/probe/people/{id}", get(role))
                .route_layer(from_fn_with_state(
                    guarded(Requirement::OwnedRole(EndpointRole::Owner)),
                    gate,
                )),
        )
}

fn get_from(uri: &str, jar: &Jar, peer: &str, forwarded: Option<&str>) -> Request {
    let mut builder = Request::builder().method(Method::GET).uri(uri);
    if let Some(cookie) = jar.header() {
        builder = builder.header(header::COOKIE, cookie);
    }
    if let Some(chain) = forwarded {
        builder = builder.header("x-forwarded-for", chain);
    }
    let mut request = builder.body(Body::empty()).expect("the request is valid");
    let address = SocketAddr::new(peer.parse().expect("a valid peer"), 50000);
    request.extensions_mut().insert(ConnectInfo(address));
    request
}

fn defaults() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: whl_core::response::ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
    }
}

async fn signed_in(harness: &Harness) -> (Jar, whl_store::Uuid) {
    let (reply, jar) = harness.sign_in(None).await;
    assert_eq!(reply.status, StatusCode::FOUND);
    let id: whl_store::Uuid = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(harness.db.pool())
        .await
        .expect("the signed-in user exists");
    (jar, id)
}

fn mixed(harness: &Harness) -> AccessControl {
    AccessControl::mixed(harness.auth.clone(), RETENTION)
}

#[tokio::test]
async fn a_session_resolves_to_a_signed_in_caller_in_mixed_mode_and_no_cookie_to_an_anonymous_one()
{
    let harness = common::harness().await;
    let app = probes(&mixed(&harness));
    let (jar, _) = signed_in(&harness).await;

    let anonymous = send(
        &app,
        get_from("/probe/anyone", &Jar::default(), "203.0.113.5", None),
    )
    .await;
    let session = send(&app, get_from("/probe/anyone", &jar, "203.0.113.5", None)).await;

    assert_eq!(anonymous.status, StatusCode::OK);
    assert_eq!(anonymous.json()["email"], Value::Null);
    assert_eq!(anonymous.json()["network"], "203.0.113.0/24");
    assert!(session.json()["email"].is_string());
    assert_eq!(session.json()["ip"], "203.0.113.5");
}

#[tokio::test]
async fn sign_in_is_reported_available_once_discovery_has_run() {
    let ready = common::harness().await;
    let waiting = common::harness_with(common::Options {
        establish: false,
        ..common::Options::default()
    })
    .await;

    assert!(mixed(&ready).sign_in_available());
    assert!(!mixed(&waiting).sign_in_available());
    assert!(!AccessControl::open().sign_in_available());
    assert_eq!(mixed(&ready).mode(), AccessMode::Mixed);
}

#[tokio::test]
async fn a_signed_in_only_route_refuses_anonymous_callers_with_the_mode_specific_answer() {
    let harness = common::harness().await;
    let (jar, _) = signed_in(&harness).await;
    let mixed_app = probes(&mixed(&harness));
    let sso_app = probes(&AccessControl::sso(harness.auth.clone(), RETENTION));

    let mixed_anonymous = send(
        &mixed_app,
        get_from("/probe/signed-in", &Jar::default(), "203.0.113.5", None),
    )
    .await;
    let sso_anonymous = send(
        &sso_app,
        get_from("/probe/signed-in", &Jar::default(), "203.0.113.5", None),
    )
    .await;
    let mixed_session = send(
        &mixed_app,
        get_from("/probe/signed-in", &jar, "203.0.113.5", None),
    )
    .await;

    assert_eq!(mixed_anonymous.status, StatusCode::FORBIDDEN);
    assert_eq!(mixed_anonymous.json()["error"]["code"], "sign_in_required");
    assert_eq!(sso_anonymous.status, StatusCode::UNAUTHORIZED);
    assert_eq!(sso_anonymous.json()["error"]["code"], "unauthenticated");
    assert_eq!(mixed_session.status, StatusCode::OK);
}

#[tokio::test]
async fn roles_resolve_per_endpoint_kind_for_anonymous_and_signed_in_callers() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness));
    let (jar, user) = signed_in(&harness).await;
    let ownerless = harness
        .store
        .create_endpoint_owned(None, &defaults(), RETENTION)
        .await
        .expect("the endpoint is stored");
    let mine = harness
        .store
        .create_endpoint_owned(Some(user), &defaults(), RETENTION)
        .await
        .expect("the endpoint is stored");
    let stranger_owner: whl_store::Uuid = sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email) VALUES (gen_random_uuid(), 'i', 's', 'x@example.test') RETURNING id",
    )
    .fetch_one(harness.db.pool())
    .await
    .expect("the other user is stored");
    let theirs = harness
        .store
        .create_endpoint_owned(Some(stranger_owner), &defaults(), RETENTION)
        .await
        .expect("the endpoint is stored");
    let open_id = ownerless.id.unpack();
    let mine_id = mine.id.unpack();
    let theirs_id = theirs.id.unpack();
    let none = Jar::default();

    let cases: [(&str, &Jar, &str, StatusCode, Option<&str>); 6] = [
        (
            "anonymous on ownerless",
            &none,
            open_id,
            StatusCode::OK,
            Some("owner"),
        ),
        (
            "signed in on ownerless",
            &jar,
            open_id,
            StatusCode::OK,
            Some("owner"),
        ),
        (
            "anonymous on owned",
            &none,
            theirs_id,
            StatusCode::GONE,
            None,
        ),
        (
            "signed in on foreign",
            &jar,
            theirs_id,
            StatusCode::GONE,
            None,
        ),
        ("owner on own", &jar, mine_id, StatusCode::OK, Some("owner")),
        ("anonymous on own", &none, mine_id, StatusCode::GONE, None),
    ];
    for (label, cookies, endpoint, status, expected) in cases {
        let reply = send(
            &app,
            get_from(
                &format!("/probe/viewer/{endpoint}"),
                cookies,
                "203.0.113.5",
                None,
            ),
        )
        .await;
        assert_eq!(reply.status, status, "{label}");
        if let Some(expected) = expected {
            assert_eq!(reply.json()["role"], expected, "{label}");
        }
    }
}

#[tokio::test]
async fn person_only_role_routes_need_a_session_and_an_owned_endpoint() {
    let harness = common::harness().await;
    let app = probes(&mixed(&harness));
    let (jar, user) = signed_in(&harness).await;
    let ownerless = harness
        .store
        .create_endpoint_owned(None, &defaults(), RETENTION)
        .await
        .expect("the endpoint is stored");
    let mine = harness
        .store
        .create_endpoint_owned(Some(user), &defaults(), RETENTION)
        .await
        .expect("the endpoint is stored");
    let none = Jar::default();

    let anonymous = send(
        &app,
        get_from(
            &format!("/probe/people/{}", mine.id.unpack()),
            &none,
            "203.0.113.5",
            None,
        ),
    )
    .await;
    let unowned = send(
        &app,
        get_from(
            &format!("/probe/people/{}", ownerless.id.unpack()),
            &jar,
            "203.0.113.5",
            None,
        ),
    )
    .await;
    let owned = send(
        &app,
        get_from(
            &format!("/probe/people/{}", mine.id.unpack()),
            &jar,
            "203.0.113.5",
            None,
        ),
    )
    .await;

    assert_eq!(anonymous.status, StatusCode::FORBIDDEN);
    assert_eq!(anonymous.json()["error"]["code"], "sign_in_required");
    assert_eq!(unowned.status, StatusCode::CONFLICT);
    assert_eq!(unowned.json()["error"]["code"], "claim_required");
    assert_eq!(owned.status, StatusCode::OK);
}

#[tokio::test]
async fn the_caller_address_comes_from_the_forwarded_chain_only_for_trusted_peers() {
    let harness = common::harness().await;
    let proxies = TrustedProxies::new(vec!["10.0.0.0/8".parse().expect("a CIDR")]);
    let app = probes(&mixed(&harness).with_proxies(proxies));
    let none = Jar::default();

    let trusted = send(
        &app,
        get_from("/probe/anyone", &none, "10.0.0.9", Some("198.51.100.4")),
    )
    .await;
    let spoofed = send(
        &app,
        get_from("/probe/anyone", &none, "203.0.113.5", Some("198.51.100.4")),
    )
    .await;

    assert_eq!(trusted.json()["ip"], "198.51.100.4");
    assert_eq!(spoofed.json()["ip"], "203.0.113.5");
}

#[tokio::test]
async fn the_debug_output_of_access_control_names_only_the_mode() {
    let harness = common::harness().await;
    let rendered = format!("{:?}", mixed(&harness));
    assert_eq!(rendered, "AccessControl { mode: \"mixed\" }");
}
