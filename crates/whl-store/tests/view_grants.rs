use std::time::Duration;

use sqlx::PgPool;
use sqlx::types::Uuid;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::{
    EndpointChange, EndpointSettings, LifecycleError, LifecycleLimits, MAX_GRANTS_PER_ENDPOINT,
    Overflow, Store, ViewPasswordChange, ViewPasswordHash, ViewStanding,
};
use whl_testkit::TestDb;

const DOWN_MIGRATION: &str =
    include_str!("../migrations/1795254382601-add-view-passwords.down.sql");
const RETENTION: Duration = Duration::from_secs(3600);
const LIFETIME: Duration = Duration::from_secs(12 * 3600);

fn settings() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
    }
}

fn hash(label: &str) -> ViewPasswordHash {
    ViewPasswordHash::new(format!("stub-hash-{label}"))
}

fn digest(seed: u8) -> [u8; 32] {
    [seed; 32]
}

async fn anonymous_endpoint(store: &Store) -> EndpointId {
    store
        .create_endpoint(&settings(), RETENTION)
        .await
        .expect("the endpoint is stored")
        .id
}

async fn change(
    store: &Store,
    id: &EndpointId,
    view: ViewPasswordChange<'_>,
) -> Result<(), LifecycleError> {
    store
        .update_endpoint_with_view_password(
            id,
            RETENTION,
            LifecycleLimits::NONE,
            view,
            |current, _owner| {
                Ok::<_, LifecycleError>(EndpointChange {
                    settings: current.settings.clone(),
                    lifecycle: current.lifecycle,
                })
            },
        )
        .await?
        .expect("the endpoint is live");
    Ok(())
}

async fn grant_count(pool: &PgPool, id: &EndpointId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM endpoint_grants WHERE endpoint_id = $1")
        .bind(id.unpack())
        .fetch_one(pool)
        .await
        .expect("the grants are counted")
}

async fn standing(store: &Store, id: &EndpointId, grant: Option<&[u8]>) -> ViewStanding {
    store
        .view_standing(id, grant, RETENTION)
        .await
        .expect("the standing is read")
}

#[tokio::test]
async fn a_grant_is_issued_only_while_the_password_hash_is_unchanged_and_a_change_revokes_it() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    assert_eq!(standing(&store, &id, None).await, ViewStanding::Open);
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    assert_eq!(standing(&store, &id, None).await, ViewStanding::Locked);

    let issued = store
        .issue_view_grant(&id, &hash("one"), &digest(1), LIFETIME, RETENTION)
        .await
        .expect("the grant query runs");
    assert!(issued.is_some());
    assert_eq!(
        standing(&store, &id, Some(&digest(1))).await,
        ViewStanding::Granted
    );
    assert_eq!(
        standing(&store, &id, Some(&digest(2))).await,
        ViewStanding::Locked
    );

    change(&store, &id, ViewPasswordChange::Set(&hash("two")))
        .await
        .expect("the password changes");
    assert_eq!(grant_count(db.pool(), &id).await, 0);
    assert_eq!(
        standing(&store, &id, Some(&digest(1))).await,
        ViewStanding::Locked
    );
    let stale = store
        .issue_view_grant(&id, &hash("one"), &digest(3), LIFETIME, RETENTION)
        .await
        .expect("the grant query runs");
    assert!(stale.is_none(), "a grant for the old password is refused");
    assert_eq!(grant_count(db.pool(), &id).await, 0);
}

#[tokio::test]
async fn removing_the_password_opens_the_endpoint_and_revokes_every_grant() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    store
        .issue_view_grant(&id, &hash("one"), &digest(1), LIFETIME, RETENTION)
        .await
        .expect("the grant query runs");

    change(&store, &id, ViewPasswordChange::Remove)
        .await
        .expect("the password is removed");

    assert_eq!(grant_count(db.pool(), &id).await, 0);
    assert_eq!(standing(&store, &id, None).await, ViewStanding::Open);
}

#[tokio::test]
async fn an_endpoint_keeps_at_most_the_newest_grants() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    let extra = 5_u8;
    let total = u8::try_from(MAX_GRANTS_PER_ENDPOINT).expect("the cap fits a byte") + extra;
    for seed in 1..=total {
        store
            .issue_view_grant(&id, &hash("one"), &digest(seed), LIFETIME, RETENTION)
            .await
            .expect("the grant query runs")
            .expect("the grant is issued");
    }

    assert_eq!(grant_count(db.pool(), &id).await, MAX_GRANTS_PER_ENDPOINT);
    assert_eq!(
        standing(&store, &id, Some(&digest(total))).await,
        ViewStanding::Granted
    );
    assert_eq!(
        standing(&store, &id, Some(&digest(1))).await,
        ViewStanding::Locked
    );
}

#[tokio::test]
async fn an_expired_grant_does_not_open_the_endpoint_and_the_purge_deletes_it() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    for seed in [1_u8, 2] {
        store
            .issue_view_grant(&id, &hash("one"), &digest(seed), LIFETIME, RETENTION)
            .await
            .expect("the grant query runs");
    }
    sqlx::query(
        "UPDATE endpoint_grants SET expires_at = now() - interval '1 second' WHERE token_hash = $1",
    )
    .bind(digest(1).as_slice())
    .execute(db.pool())
    .await
    .expect("the grant is aged");

    assert_eq!(
        standing(&store, &id, Some(&digest(1))).await,
        ViewStanding::Locked
    );
    assert_eq!(
        standing(&store, &id, Some(&digest(2))).await,
        ViewStanding::Granted
    );
    assert_eq!(
        store
            .delete_expired_view_grants()
            .await
            .expect("the purge runs"),
        1
    );
    assert_eq!(grant_count(db.pool(), &id).await, 1);
}

#[tokio::test]
async fn an_endpoint_with_an_owner_cannot_carry_a_view_password() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner: Uuid = sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, 'user@example.test') \
         RETURNING id",
    )
    .fetch_one(db.pool())
    .await
    .expect("the user inserts");
    let id = store
        .create_endpoint_owned(Some(owner), &settings(), RETENTION)
        .await
        .expect("the endpoint is stored")
        .id;

    let refused = change(&store, &id, ViewPasswordChange::Set(&hash("one"))).await;
    let direct =
        sqlx::query("UPDATE endpoints SET view_password_hash = 'stub-hash-direct' WHERE id = $1")
            .bind(id.unpack())
            .execute(db.pool())
            .await;

    assert!(refused.is_err());
    assert!(direct.is_err(), "the constraint holds below the API");
    assert_eq!(standing(&store, &id, None).await, ViewStanding::Open);
}

#[tokio::test]
async fn the_least_recently_used_grant_is_evicted_and_not_the_oldest_by_creation() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    let full = u8::try_from(MAX_GRANTS_PER_ENDPOINT).expect("the cap fits a byte");
    for seed in 1..=full {
        store
            .issue_view_grant(&id, &hash("one"), &digest(seed), LIFETIME, RETENTION)
            .await
            .expect("the grant query runs")
            .expect("the grant is issued");
    }
    sqlx::query(
        "UPDATE endpoint_grants SET last_used_at = now() - interval '1 hour' WHERE token_hash <> $1",
    )
    .bind(digest(1).as_slice())
    .execute(db.pool())
    .await
    .expect("the other grants are aged");

    store
        .issue_view_grant(&id, &hash("one"), &digest(full + 1), LIFETIME, RETENTION)
        .await
        .expect("the grant query runs")
        .expect("the grant is issued");

    assert_eq!(grant_count(db.pool(), &id).await, MAX_GRANTS_PER_ENDPOINT);
    for (seed, expected) in [
        (1, ViewStanding::Granted),
        (2, ViewStanding::Locked),
        (full + 1, ViewStanding::Granted),
    ] {
        assert_eq!(
            standing(&store, &id, Some(&digest(seed))).await,
            expected,
            "grant {seed}"
        );
    }
}

#[tokio::test]
async fn using_a_grant_refreshes_its_last_use_at_most_once_a_minute() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");
    store
        .issue_view_grant(&id, &hash("one"), &digest(1), LIFETIME, RETENTION)
        .await
        .expect("the grant query runs");
    let used_within = |seconds: i32| {
        let pool = db.pool().clone();
        async move {
            sqlx::query_scalar::<_, bool>(
                "SELECT last_used_at > now() - make_interval(secs => $1) FROM endpoint_grants",
            )
            .bind(f64::from(seconds))
            .fetch_one(&pool)
            .await
            .expect("the last use is read")
        }
    };

    sqlx::query("UPDATE endpoint_grants SET last_used_at = now() - interval '5 minutes'")
        .execute(db.pool())
        .await
        .expect("the grant is aged");
    assert_eq!(
        standing(&store, &id, Some(&digest(1))).await,
        ViewStanding::Granted
    );
    assert!(used_within(10).await, "an old use is refreshed");

    sqlx::query("UPDATE endpoint_grants SET last_used_at = now() - interval '30 seconds'")
        .execute(db.pool())
        .await
        .expect("the grant is aged");
    standing(&store, &id, Some(&digest(1))).await;
    assert!(!used_within(20).await, "a recent use is left alone");
}

#[tokio::test]
async fn rolling_back_is_refused_while_an_endpoint_still_has_a_view_password() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let id = anonymous_endpoint(&store).await;
    change(&store, &id, ViewPasswordChange::Set(&hash("one")))
        .await
        .expect("the password is set");

    let refused = sqlx::raw_sql(DOWN_MIGRATION).execute(db.pool()).await;

    let message = refused.expect_err("the rollback is refused").to_string();
    assert!(message.contains("cannot roll back"), "{message}");
    assert_eq!(standing(&store, &id, None).await, ViewStanding::Locked);
    change(&store, &id, ViewPasswordChange::Remove)
        .await
        .expect("the password is removed");
    sqlx::raw_sql(DOWN_MIGRATION)
        .execute(db.pool())
        .await
        .expect("the rollback runs once nothing is protected");
}
