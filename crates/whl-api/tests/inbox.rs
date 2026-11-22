mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::matrix::{Cast, cast, store_capture, store_capture_at};
use common::tokens::{ALL_SCOPES, mint};
use common::{Actor, SsoHarness, UNKNOWN_ID, field_codes, mixed_harness, sso_harness};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_core::endpoint_id::EndpointId;
use whl_live::SeqVector;
use whl_store::GrantedRole;

const TICK: Duration = Duration::from_millis(150);
const NEVER: Duration = Duration::from_secs(3600);

struct Scene {
    cast: Cast,
    first: String,
    second: String,
}

async fn scene(harness: &SsoHarness) -> Scene {
    let cast = cast(harness).await;
    let first = harness.endpoint_for(&cast.owner).await;
    let second = harness.endpoint_for(&cast.owner).await;
    for endpoint in [&first, &second] {
        let id = EndpointId::parse(endpoint).expect("a valid endpoint id");
        harness
            .store
            .set_member(
                &id,
                common::RETENTION,
                &cast.viewer.email,
                GrantedRole::Viewer,
            )
            .await
            .expect("the member is stored");
    }
    Scene {
        cast,
        first,
        second,
    }
}

fn at(second: i64) -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1_700_000_000 + second)
}

async fn get(harness: &SsoHarness, actor: Option<&Actor>, uri: &str) -> common::Reply {
    harness.send_as(actor, Method::GET, uri, "").await
}

fn ids(reply: &common::Reply) -> Vec<String> {
    reply.json()["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|item| item["id"].as_str().expect("an id").to_owned())
        .collect()
}

async fn interleave(harness: &SsoHarness, endpoints: &[&str], rounds: i64) -> Vec<String> {
    let mut stored = Vec::new();
    for round in 0..rounds {
        for (index, endpoint) in endpoints.iter().enumerate() {
            let second = round * i64::try_from(endpoints.len()).expect("small") + index as i64;
            stored.push(store_capture_at(&harness.store, endpoint, at(second)).await);
        }
    }
    stored
}

#[tokio::test]
async fn the_merged_list_interleaves_endpoints_newest_first() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let mut expected = interleave(&harness, &[&scene.first, &scene.second], 3).await;
    expected.reverse();

    let reply = get(
        &harness,
        Some(&scene.cast.viewer),
        &format!(
            "/api/v1/inbox/captures?endpoints={},{}",
            scene.first, scene.second
        ),
    )
    .await;

    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(ids(&reply), expected);
    assert!(reply.json()["next_cursor"].is_null());
    assert_eq!(reply.json()["items"][0]["endpoint_id"], scene.second);
}

#[tokio::test]
async fn cursor_pages_cover_every_capture_once_in_order() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let third = harness.endpoint_for(&scene.cast.owner).await;
    let mut expected = interleave(&harness, &[&scene.first, &scene.second, &third], 7).await;
    expected.reverse();

    let mut collected = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut uri = format!(
            "/api/v1/inbox/captures?endpoints={},{},{third}&limit=4",
            scene.first, scene.second
        );
        if let Some(cursor) = &cursor {
            uri.push_str(&format!("&cursor={cursor}"));
        }
        let reply = get(&harness, Some(&scene.cast.owner), &uri).await;
        assert_eq!(reply.status, StatusCode::OK);
        collected.extend(ids(&reply));
        pages += 1;
        cursor = reply.json()["next_cursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(collected, expected);
    assert_eq!(pages, 6);
}

#[tokio::test]
async fn filters_apply_to_every_endpoint_of_the_selection() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let stored = interleave(&harness, &[&scene.first, &scene.second], 3).await;
    let since = at(4).format(&Rfc3339).expect("formats");

    let reply = get(
        &harness,
        Some(&scene.cast.owner),
        &format!(
            "/api/v1/inbox/captures?endpoints={},{}&since={since}",
            scene.first, scene.second
        ),
    )
    .await;

    assert_eq!(ids(&reply), [stored[5].clone(), stored[4].clone()]);
}

#[tokio::test]
async fn a_cursor_is_refused_under_other_filters_or_another_selection() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    interleave(&harness, &[&scene.first, &scene.second], 3).await;
    let both = format!("{},{}", scene.first, scene.second);
    let page = get(
        &harness,
        Some(&scene.cast.owner),
        &format!("/api/v1/inbox/captures?endpoints={both}&limit=2"),
    )
    .await;
    let cursor = page.json()["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();

    for uri in [
        format!("/api/v1/inbox/captures?endpoints={both}&limit=2&method=GET&cursor={cursor}"),
        format!(
            "/api/v1/inbox/captures?endpoints={}&limit=2&cursor={cursor}",
            scene.first
        ),
        format!("/api/v1/inbox/captures?endpoints={both}&cursor=!!"),
    ] {
        let reply = get(&harness, Some(&scene.cast.owner), &uri).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(reply.json()["error"]["code"], "invalid_cursor");
    }
}

#[tokio::test]
async fn one_endpoint_without_a_role_refuses_the_selection_with_the_uniform_gone() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let foreign = harness.endpoint_for(&scene.cast.stranger).await;
    store_capture(&harness.store, &foreign).await;
    store_capture(&harness.store, &scene.first).await;
    let viewer = Some(&scene.cast.viewer);

    let alone = get(
        &harness,
        viewer,
        &format!("/api/v1/inbox/captures?endpoints={}", scene.first),
    )
    .await;
    let denied = get(
        &harness,
        viewer,
        &format!("/api/v1/inbox/captures?endpoints={},{foreign}", scene.first),
    )
    .await;
    let unknown = get(
        &harness,
        viewer,
        &format!(
            "/api/v1/inbox/captures?endpoints={},{UNKNOWN_ID}",
            scene.first
        ),
    )
    .await;

    assert_eq!(alone.status, StatusCode::OK);
    assert_eq!(denied.status, StatusCode::GONE);
    assert_eq!(denied.json()["error"]["code"], "endpoint_gone");
    assert_eq!(denied.json(), unknown.json());
    assert_eq!(denied.status, unknown.status);
}

#[tokio::test]
async fn without_a_selection_the_inbox_covers_own_and_shared_endpoints_only() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let foreign = harness.endpoint_for(&scene.cast.stranger).await;
    let own = store_capture(&harness.store, &scene.first).await;
    let shared = store_capture(&harness.store, &scene.second).await;
    store_capture(&harness.store, &foreign).await;

    let reply = get(&harness, Some(&scene.cast.viewer), "/api/v1/inbox/captures").await;
    let anonymous = get(&harness, None, "/api/v1/inbox/captures").await;

    let mut found = ids(&reply);
    found.sort();
    let mut expected = vec![own, shared];
    expected.sort();
    assert_eq!(found, expected);
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_selection_is_bounded_and_folders_are_not_available_yet() {
    let harness = mixed_harness().await;
    let signed_in = harness.actor("sig", "signed@example.test", true).await;
    let mut anonymous_ids = Vec::new();
    for _ in 0..21 {
        anonymous_ids.push(harness.anonymous_endpoint(None).await);
    }
    let fifty_one: Vec<String> = (0..51).map(|index| format!("{index:024}")).collect();

    let twenty = get(
        &harness,
        None,
        &format!(
            "/api/v1/inbox/captures?endpoints={}",
            anonymous_ids[..20].join(",")
        ),
    )
    .await;
    let twenty_one = get(
        &harness,
        None,
        &format!(
            "/api/v1/inbox/captures?endpoints={}",
            anonymous_ids.join(",")
        ),
    )
    .await;
    let over_fifty = get(
        &harness,
        Some(&signed_in),
        &format!("/api/v1/inbox/captures?endpoints={}", fifty_one.join(",")),
    )
    .await;
    let folder = get(
        &harness,
        Some(&signed_in),
        "/api/v1/inbox/captures?folder=anything",
    )
    .await;

    assert_eq!(twenty.status, StatusCode::OK);
    for refused in [&twenty_one, &over_fifty] {
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            field_codes(&refused.json()),
            [("endpoints".to_owned(), "too_many".to_owned())]
        );
    }
    assert_eq!(
        field_codes(&folder.json()),
        [("folder".to_owned(), "not_allowed".to_owned())]
    );
}

fn endpoint_and_seq(event: &serde_json::Value) -> (String, i64) {
    (
        event["endpoint_id"]
            .as_str()
            .expect("an endpoint")
            .to_owned(),
        event["seq"].as_i64().expect("a seq"),
    )
}

#[tokio::test]
async fn the_merged_stream_resumes_from_the_vector_without_gaps() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let app = harness.live_app(NEVER).await;
    let query = format!("endpoints={},{}", scene.first, scene.second);
    let mut stream = app.open_inbox(Some(&scene.cast.owner), &query, None).await;
    assert_eq!(stream.status, StatusCode::OK);
    let ready = stream.expect_event("ready").await;
    assert_eq!(ready["endpoints"].as_array().expect("endpoints").len(), 2);

    store_capture(&harness.store, &scene.first).await;
    store_capture(&harness.store, &scene.second).await;
    store_capture(&harness.store, &scene.first).await;
    let mut seen = Vec::new();
    let mut resume_from = None;
    for index in 0..3 {
        seen.push(endpoint_and_seq(&stream.expect_event("capture").await));
        if index == 1 {
            resume_from = stream.last_id.clone();
        }
    }
    drop(stream);
    store_capture(&harness.store, &scene.second).await;
    store_capture(&harness.store, &scene.first).await;

    let mut resumed = app
        .open_inbox(Some(&scene.cast.owner), &query, resume_from.as_deref())
        .await;
    resumed.expect_event("ready").await;
    let mut replayed = Vec::new();
    for _ in 0..3 {
        replayed.push(endpoint_and_seq(&resumed.expect_event("capture").await));
    }
    store_capture(&harness.store, &scene.second).await;
    let live = endpoint_and_seq(&resumed.expect_event("capture").await);

    let first = scene.first.clone();
    let second = scene.second.clone();
    assert_eq!(seen[2], (first.clone(), 2));
    let mut sorted = replayed.clone();
    sorted.sort();
    let mut expected = vec![(first.clone(), 2), (second.clone(), 2), (first.clone(), 3)];
    expected.sort();
    assert_eq!(sorted, expected);
    let lane_order = |endpoint: &str| -> Vec<i64> {
        replayed
            .iter()
            .filter(|(owner, _)| owner == endpoint)
            .map(|(_, seq)| *seq)
            .collect()
    };
    assert_eq!(lane_order(&first), [2, 3]);
    assert_eq!(live, (second, 3));
    let decoded = SeqVector::decode(resumed.last_id.as_deref().expect("an id")).expect("decodes");
    assert_eq!(
        decoded.get(&EndpointId::parse(&first).expect("id")),
        Some(3)
    );
}

#[tokio::test]
async fn a_resume_past_the_replay_limit_sends_resync_instead_of_captures() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    for _ in 0..51 {
        store_capture(&harness.store, &scene.first).await;
    }
    for _ in 0..50 {
        store_capture(&harness.store, &scene.second).await;
    }
    let mut vector = SeqVector::new();
    vector.set(&EndpointId::parse(&scene.first).expect("id"), 0);
    vector.set(&EndpointId::parse(&scene.second).expect("id"), 0);
    let app = harness.live_app(NEVER).await;

    let mut stream = app
        .open_inbox(
            Some(&scene.cast.owner),
            &format!("endpoints={},{}", scene.first, scene.second),
            Some(&vector.encode()),
        )
        .await;

    stream.expect_event("ready").await;
    assert_eq!(stream.expect_event("resync").await["reason"], "gap");
}

#[tokio::test]
async fn a_selection_with_a_denied_endpoint_gets_the_uniform_gone_on_the_stream() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let foreign = harness.endpoint_for(&scene.cast.stranger).await;
    let app = harness.live_app(NEVER).await;

    let mut denied = app
        .open_inbox(
            Some(&scene.cast.viewer),
            &format!("endpoints={},{foreign}", scene.first),
            None,
        )
        .await;
    let mut unknown = app
        .open_inbox(
            Some(&scene.cast.viewer),
            &format!("endpoints={},{UNKNOWN_ID}", scene.first),
            None,
        )
        .await;

    assert_eq!(denied.status, StatusCode::GONE);
    assert_eq!(unknown.status, StatusCode::GONE);
    let body = denied.error_body().await;
    assert_eq!(body["error"]["code"], "endpoint_gone");
    assert_eq!(body, unknown.error_body().await);
}

#[tokio::test]
async fn losing_access_to_one_endpoint_ends_the_merged_stream_within_one_tick() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let app = harness.live_app(TICK).await;
    let mut stream = app
        .open_inbox(
            Some(&scene.cast.viewer),
            &format!("endpoints={},{}", scene.first, scene.second),
            None,
        )
        .await;
    stream.expect_event("ready").await;

    sqlx::query("DELETE FROM endpoint_members WHERE endpoint_id = $1 AND user_id = $2")
        .bind(&scene.second)
        .bind(scene.cast.viewer.id)
        .execute(harness.db.pool())
        .await
        .expect("the membership is deleted");

    stream.expect_end_after_bye().await;
}

#[tokio::test]
async fn merged_events_name_their_endpoint_and_endpoint_updates_are_left_out() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let app = harness.live_app(NEVER).await;
    let mut stream = app
        .open_inbox(
            Some(&scene.cast.owner),
            &format!("endpoints={},{}", scene.first, scene.second),
            None,
        )
        .await;
    stream.expect_event("ready").await;
    let capture = store_capture(&harness.store, &scene.first).await;
    stream.expect_event("capture").await;

    let deleted = harness
        .send_as(
            Some(&scene.cast.owner),
            Method::DELETE,
            &format!("/api/v1/endpoints/{}/captures/{capture}", scene.first),
            "",
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let removal = stream.expect_event("capture_deleted").await;
    assert_eq!(removal["endpoint_id"], scene.first);

    let patched = harness
        .send_as(
            Some(&scene.cast.owner),
            Method::PATCH,
            &format!("/api/v1/endpoints/{}", scene.second),
            r#"{"title":"renamed"}"#,
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK);
    store_capture(&harness.store, &scene.second).await;
    let next = stream.expect_event("capture").await;
    assert_eq!(next["endpoint_id"], scene.second);
}

#[tokio::test]
async fn a_principal_may_hold_four_inbox_streams_and_a_closed_one_frees_its_slot() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let app = harness.live_app(NEVER).await;
    let query = format!("endpoints={}", scene.first);
    let viewer = Some(&scene.cast.viewer);
    let mut open = Vec::new();
    for _ in 0..4 {
        let stream = app.open_inbox(viewer, &query, None).await;
        assert_eq!(stream.status, StatusCode::OK);
        open.push(stream);
    }

    let mut refused = app.open_inbox(viewer, &query, None).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.error_body().await["error"]["code"], "rate_limited");

    drop(open.pop());
    let mut freed = false;
    for _ in 0..100 {
        let stream = app.open_inbox(viewer, &query, None).await;
        if stream.status == StatusCode::OK {
            freed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(freed);
}

#[tokio::test]
async fn a_cursor_survives_a_change_of_the_default_selection() {
    let harness = sso_harness().await;
    let cast = cast(&harness).await;
    let mut owned = Vec::new();
    for _ in 0..51 {
        owned.push(harness.endpoint_for(&cast.owner).await);
    }
    let mut expected = Vec::new();
    for second in 0..3 {
        expected.push(store_capture_at(&harness.store, &owned[1], at(second)).await);
    }
    expected.reverse();

    let first = get(
        &harness,
        Some(&cast.owner),
        "/api/v1/inbox/captures?limit=2",
    )
    .await;
    assert_eq!(ids(&first), expected[..2]);
    let cursor = first.json()["next_cursor"]
        .as_str()
        .expect("a cursor")
        .to_owned();
    store_capture(&harness.store, &owned[0]).await;

    let second = get(
        &harness,
        Some(&cast.owner),
        &format!("/api/v1/inbox/captures?limit=2&cursor={cursor}"),
    )
    .await;

    assert_eq!(second.status, StatusCode::OK);
    assert_eq!(ids(&second), expected[2..]);
    assert!(second.json()["next_cursor"].is_null());
}

#[tokio::test]
async fn an_access_change_on_one_endpoint_rechecks_only_that_endpoint() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let first = EndpointId::parse(&scene.first).expect("a valid endpoint id");
    harness
        .store
        .set_member(
            &first,
            common::RETENTION,
            &scene.cast.editor.email,
            GrantedRole::Editor,
        )
        .await
        .expect("the member is stored");
    let app = harness.live_app(NEVER).await;
    let mut stream = app
        .open_inbox(
            Some(&scene.cast.viewer),
            &format!("endpoints={},{}", scene.first, scene.second),
            None,
        )
        .await;
    stream.expect_event("ready").await;
    sqlx::query("DELETE FROM endpoint_members WHERE endpoint_id = $1 AND user_id = $2")
        .bind(&scene.second)
        .bind(scene.cast.viewer.id)
        .execute(harness.db.pool())
        .await
        .expect("the membership is deleted");

    let removed = harness
        .send_as(
            Some(&scene.cast.owner),
            Method::DELETE,
            &format!(
                "/api/v1/endpoints/{}/members/{}",
                scene.first,
                scene.cast.editor.id.hyphenated()
            ),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    store_capture(&harness.store, &scene.first).await;

    assert_eq!(stream.expect_event("capture").await["seq"], 1);
}

#[tokio::test]
async fn an_api_token_is_refused_by_the_inbox_stream_exactly_as_by_the_inbox_list() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let minted = mint(&harness, &scene.cast.owner, &ALL_SCOPES, None).await;
    let app = harness.live_app(NEVER).await;
    let query = format!("endpoints={}", scene.first);

    let list = harness
        .send_bearer(
            &minted.token,
            Method::GET,
            &format!("/api/v1/inbox/captures?{query}"),
            "",
        )
        .await;
    let mut stream = app.open_inbox_bearer(&minted.token, &query).await;
    let mut default_selection = app.open_inbox_bearer(&minted.token, "").await;

    assert_eq!(list.status, StatusCode::FORBIDDEN);
    assert_eq!(stream.status, StatusCode::FORBIDDEN);
    assert_eq!(default_selection.status, StatusCode::FORBIDDEN);
    assert_eq!(list.json()["error"]["code"], "session_required");
    assert_eq!(stream.error_body().await, list.json());
    assert_eq!(default_selection.error_body().await, list.json());
}
