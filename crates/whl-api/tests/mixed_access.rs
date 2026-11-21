mod common;

use axum::http::{Method, StatusCode};
use common::matrix::{Cast, Fixture, Route, cast, error_code, fixture, routes, snapshot, uri};
use common::{Actor, SsoHarness, UNKNOWN_ID, mixed_harness};
use whl_store::EndpointRole;

#[derive(Debug, PartialEq, Eq)]
enum Expected {
    Success(StatusCode),
    Refused(StatusCode, &'static str),
}

fn held_role(owned: bool, caller: &str) -> Option<EndpointRole> {
    if !owned {
        return Some(EndpointRole::Owner);
    }
    match caller {
        "viewer" => Some(EndpointRole::Viewer),
        "editor" => Some(EndpointRole::Editor),
        "owner" => Some(EndpointRole::Owner),
        _ => None,
    }
}

fn expected(route: &Route, owned: bool, caller: &str) -> Expected {
    if route.members && caller == "anonymous" {
        return Expected::Refused(StatusCode::FORBIDDEN, "sign_in_required");
    }
    match held_role(owned, caller) {
        None => Expected::Refused(StatusCode::GONE, "endpoint_gone"),
        Some(held) if held < route.needed => Expected::Refused(StatusCode::FORBIDDEN, "forbidden"),
        Some(_) if route.members && !owned => {
            Expected::Refused(StatusCode::CONFLICT, "claim_required")
        }
        Some(_) => Expected::Success(route.success),
    }
}

fn callers(cast: &Cast) -> [(&'static str, Option<&Actor>); 5] {
    [
        ("anonymous", None),
        ("stranger", Some(&cast.stranger)),
        ("viewer", Some(&cast.viewer)),
        ("editor", Some(&cast.editor)),
        ("owner", Some(&cast.owner)),
    ]
}

async fn run_matrix(harness: &SsoHarness, owned: bool) {
    let cast = cast(harness).await;
    for route in routes() {
        for (name, actor) in callers(&cast) {
            let fixture = fixture(harness, &cast, owned).await;
            let before = snapshot(harness, &fixture.endpoint).await;
            let target = uri(&route, &fixture, &cast);
            let reply = harness
                .send_as(actor, route.method.clone(), &target, route.body)
                .await;
            let label = format!(
                "{} {} as {name} on an {} endpoint",
                route.method,
                route.path,
                if owned { "owned" } else { "ownerless" }
            );
            match expected(&route, owned, name) {
                Expected::Success(status) => {
                    assert_eq!(reply.status, status, "{label}");
                    continue;
                }
                Expected::Refused(status, code) => {
                    assert_eq!(reply.status, status, "{label}");
                    assert_eq!(error_code(&reply), code, "{label}");
                }
            }
            assert_eq!(
                snapshot(harness, &fixture.endpoint).await,
                before,
                "{label} left the data untouched"
            );
        }
    }
}

#[tokio::test]
async fn on_an_ownerless_endpoint_every_caller_holds_the_owner_role_except_on_member_routes() {
    let harness = mixed_harness().await;
    run_matrix(&harness, false).await;
}

#[tokio::test]
async fn on_an_owned_endpoint_the_sso_role_matrix_holds_and_anonymous_callers_have_no_role() {
    let harness = mixed_harness().await;
    run_matrix(&harness, true).await;
}

#[tokio::test]
async fn an_undecodable_endpoint_id_is_not_found_like_a_malformed_one_for_every_caller() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let actors = [
        None,
        Some(&cast.stranger),
        Some(&cast.viewer),
        Some(&cast.editor),
        Some(&cast.owner),
    ];
    common::matrix::assert_undecodable_ids_are_not_found(&harness, &actors).await;
    common::matrix::assert_undecodable_claim_is_not_found_once_signed_in(&harness, &cast.stranger)
        .await;
}

async fn owned_fixture(harness: &SsoHarness) -> (Cast, Fixture) {
    let cast = cast(harness).await;
    let fixture = fixture(harness, &cast, true).await;
    (cast, fixture)
}

#[tokio::test]
async fn an_anonymous_caller_cannot_read_or_change_an_owned_endpoint_even_with_its_id() {
    let harness = mixed_harness().await;
    let (_, owned) = owned_fixture(&harness).await;
    let before = snapshot(&harness, &owned.endpoint).await;
    let base = format!("/api/v1/endpoints/{}", owned.endpoint);
    let unknown = format!("/api/v1/endpoints/{UNKNOWN_ID}");
    let attempts = [
        (Method::GET, "", ""),
        (Method::PATCH, "", r#"{"title":"taken over"}"#),
        (Method::DELETE, "", ""),
        (Method::GET, "/captures", ""),
        (Method::GET, "/captures/latest", ""),
        (Method::GET, "/forward-rules", ""),
        (
            Method::POST,
            "/forward-rules",
            r#"{"target_url":"http://8.8.8.8/x"}"#,
        ),
        (Method::DELETE, "/captures", ""),
    ];
    for (method, suffix, body) in attempts {
        let real = harness
            .send_as(None, method.clone(), &format!("{base}{suffix}"), body)
            .await;
        let missing = harness
            .send_as(None, method.clone(), &format!("{unknown}{suffix}"), body)
            .await;
        assert_eq!(real.status, StatusCode::GONE, "{method} {suffix}");
        assert_eq!(real.body, missing.body, "{method} {suffix}");
    }
    assert_eq!(snapshot(&harness, &owned.endpoint).await, before);
}

#[tokio::test]
async fn a_stale_or_forged_session_cookie_gives_no_more_than_anonymous_access() {
    let harness = mixed_harness().await;
    let (cast, owned) = owned_fixture(&harness).await;
    let target = format!("/api/v1/endpoints/{}", owned.endpoint);
    let forged = Actor {
        cookie: format!("{}={:x<43}", common::SESSION_COOKIE, ""),
        ..cast.owner.clone()
    };
    let reply = harness
        .send_as(Some(&forged), Method::GET, &target, "")
        .await;
    assert_eq!(reply.status, StatusCode::GONE);
    let ownerless = harness.anonymous_endpoint(None).await;
    let open_door = harness
        .send_as(
            Some(&forged),
            Method::GET,
            &format!("/api/v1/endpoints/{ownerless}"),
            "",
        )
        .await;
    assert_eq!(open_door.status, StatusCode::OK);
    assert_eq!(open_door.json()["my_role"], "owner");
    assert!(open_door.json()["owner"].is_null());
}

#[tokio::test]
async fn an_ownerless_endpoint_stays_fully_usable_without_signing_in() {
    let harness = mixed_harness().await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let target = format!("/api/v1/endpoints/{endpoint}");
    let patched = harness
        .send_as(None, Method::PATCH, &target, r#"{"title":"anon"}"#)
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    assert_eq!(patched.json()["title"], "anon");
    assert!(patched.json()["owner"].is_null());
    let removed = harness.send_as(None, Method::DELETE, &target, "").await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    let gone = harness.send_as(None, Method::GET, &target, "").await;
    assert_eq!(gone.status, StatusCode::GONE);
}

#[tokio::test]
async fn a_signed_in_stranger_cannot_add_members_to_an_ownerless_endpoint() {
    let harness = mixed_harness().await;
    let cast = cast(&harness).await;
    let endpoint = harness.anonymous_endpoint(None).await;
    let reply = harness
        .send_as(
            Some(&cast.stranger),
            Method::PUT,
            &format!("/api/v1/endpoints/{endpoint}/members"),
            r#"{"email":"viewer@example.test","role":"editor"}"#,
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(error_code(&reply), "claim_required");
    let members: i64 = sqlx::query_scalar("SELECT count(*) FROM endpoint_members")
        .fetch_one(harness.db.pool())
        .await
        .expect("the count runs");
    assert_eq!(members, 0);
}
