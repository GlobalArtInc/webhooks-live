mod common;

use std::time::Duration;

use axum::http::{Method, StatusCode};
use common::matrix::{Cast, cast, store_capture};
use common::{Actor, SsoHarness, mixed_harness, sso_harness};
use sha2::{Digest, Sha256};
use whl_core::endpoint_id::EndpointId;
use whl_store::GrantedRole;

const TICK: Duration = Duration::from_millis(150);
const NEVER: Duration = Duration::from_secs(3600);

struct Scene {
    cast: Cast,
    endpoint: String,
}

async fn scene(harness: &SsoHarness) -> Scene {
    let cast = cast(harness).await;
    let endpoint = harness.endpoint_for(&cast.owner).await;
    let id = EndpointId::parse(&endpoint).expect("a valid endpoint id");
    for (actor, role) in [
        (&cast.editor, GrantedRole::Editor),
        (&cast.viewer, GrantedRole::Viewer),
    ] {
        harness
            .store
            .set_member(&id, common::RETENTION, &actor.email, role)
            .await
            .expect("the member is stored");
    }
    Scene { cast, endpoint }
}

async fn open(app: &common::LiveApp, actor: &Actor, endpoint: &str) -> common::Sse {
    let mut stream = app.open(Some(actor), endpoint).await;
    assert_eq!(stream.status, StatusCode::OK);
    stream.expect_event("ready").await;
    stream
}

async fn removal_ends_the_stream_at_once(harness: SsoHarness) {
    let scene = scene(&harness).await;
    let app = harness.live_app(NEVER).await;
    let mut viewer = open(&app, &scene.cast.viewer, &scene.endpoint).await;
    let mut editor = open(&app, &scene.cast.editor, &scene.endpoint).await;

    store_capture(&harness.store, &scene.endpoint).await;
    assert_eq!(viewer.expect_event("capture").await["seq"], 1);
    editor.expect_event("capture").await;

    let removed = harness
        .send_as(
            Some(&scene.cast.owner),
            Method::DELETE,
            &format!(
                "/api/v1/endpoints/{}/members/{}",
                scene.endpoint,
                scene.cast.viewer.id.hyphenated()
            ),
            "",
        )
        .await;
    assert_eq!(removed.status, StatusCode::NO_CONTENT);
    viewer.expect_end_after_bye().await;

    store_capture(&harness.store, &scene.endpoint).await;
    assert_eq!(editor.expect_event("capture").await["seq"], 2);
}

async fn removal_without_a_notification_ends_the_stream_within_one_tick(harness: SsoHarness) {
    let scene = scene(&harness).await;
    let app = harness.live_app(TICK).await;
    let mut viewer = open(&app, &scene.cast.viewer, &scene.endpoint).await;
    let mut editor = open(&app, &scene.cast.editor, &scene.endpoint).await;

    sqlx::query("DELETE FROM endpoint_members WHERE endpoint_id = $1 AND user_id = $2")
        .bind(&scene.endpoint)
        .bind(scene.cast.viewer.id)
        .execute(harness.db.pool())
        .await
        .expect("the membership is deleted");
    viewer.expect_end_after_bye().await;

    store_capture(&harness.store, &scene.endpoint).await;
    assert_eq!(editor.expect_event("capture").await["seq"], 1);
}

async fn logout_ends_the_stream_within_one_tick(harness: SsoHarness) {
    let scene = scene(&harness).await;
    let app = harness.live_app(TICK).await;
    let mut viewer = open(&app, &scene.cast.viewer, &scene.endpoint).await;
    let mut editor = open(&app, &scene.cast.editor, &scene.endpoint).await;

    let token = scene
        .cast
        .viewer
        .cookie
        .split_once('=')
        .expect("a cookie pair")
        .1;
    let deleted = harness
        .store
        .delete_session(Sha256::digest(token.as_bytes()).as_slice())
        .await
        .expect("the session is deleted");
    assert!(deleted);
    viewer.expect_end_after_bye().await;

    store_capture(&harness.store, &scene.endpoint).await;
    assert_eq!(editor.expect_event("capture").await["seq"], 1);
}

async fn an_authorised_viewer_keeps_streaming_across_ticks(harness: SsoHarness) {
    let scene = scene(&harness).await;
    let app = harness.live_app(TICK).await;
    let mut viewer = open(&app, &scene.cast.viewer, &scene.endpoint).await;
    for seq in 1..=3 {
        tokio::time::sleep(TICK * 2).await;
        store_capture(&harness.store, &scene.endpoint).await;
        assert_eq!(viewer.expect_event("capture").await["seq"], seq);
    }
}

async fn a_patch_reaches_a_stream_viewer_without_per_viewer_fields(harness: SsoHarness) {
    let scene = scene(&harness).await;
    let app = harness.live_app(NEVER).await;
    let mut viewer = open(&app, &scene.cast.viewer, &scene.endpoint).await;

    let patched = harness
        .send_as(
            Some(&scene.cast.editor),
            Method::PATCH,
            &format!("/api/v1/endpoints/{}", scene.endpoint),
            r#"{"title":"renamed"}"#,
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK);

    let update = viewer.expect_event("endpoint_updated").await;
    assert_eq!(update["title"], "renamed");
    assert_eq!(update["id"], scene.endpoint);
    let keys = update.as_object().expect("the update is an object");
    assert!(!keys.contains_key("owner"));
    assert!(!keys.contains_key("my_role"));
}

#[tokio::test]
async fn sso_removal_ends_the_stream_at_once() {
    removal_ends_the_stream_at_once(sso_harness().await).await;
}

#[tokio::test]
async fn mixed_removal_ends_the_stream_at_once() {
    removal_ends_the_stream_at_once(mixed_harness().await).await;
}

#[tokio::test]
async fn sso_removal_without_a_notification_ends_the_stream_within_one_tick() {
    removal_without_a_notification_ends_the_stream_within_one_tick(sso_harness().await).await;
}

#[tokio::test]
async fn mixed_removal_without_a_notification_ends_the_stream_within_one_tick() {
    removal_without_a_notification_ends_the_stream_within_one_tick(mixed_harness().await).await;
}

#[tokio::test]
async fn sso_logout_ends_the_stream_within_one_tick() {
    logout_ends_the_stream_within_one_tick(sso_harness().await).await;
}

#[tokio::test]
async fn mixed_logout_ends_the_stream_within_one_tick() {
    logout_ends_the_stream_within_one_tick(mixed_harness().await).await;
}

#[tokio::test]
async fn sso_authorised_viewer_keeps_streaming() {
    an_authorised_viewer_keeps_streaming_across_ticks(sso_harness().await).await;
}

#[tokio::test]
async fn mixed_authorised_viewer_keeps_streaming() {
    an_authorised_viewer_keeps_streaming_across_ticks(mixed_harness().await).await;
}

#[tokio::test]
async fn sso_patch_reaches_a_stream_viewer() {
    a_patch_reaches_a_stream_viewer_without_per_viewer_fields(sso_harness().await).await;
}

#[tokio::test]
async fn a_stranger_cannot_open_a_stream_and_the_guard_answers_before_the_pump() {
    let harness = sso_harness().await;
    let scene = scene(&harness).await;
    let app = harness.live_app(TICK).await;
    let refused = app.open(Some(&scene.cast.stranger), &scene.endpoint).await;
    assert_eq!(refused.status, StatusCode::GONE);
    let anonymous = app.open(None, &scene.endpoint).await;
    assert_eq!(anonymous.status, StatusCode::GONE);
}
