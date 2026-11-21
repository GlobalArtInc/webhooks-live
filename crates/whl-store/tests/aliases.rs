use std::time::Duration;

use sqlx::PgPool;
use sqlx::types::Uuid;
use whl_core::alias::Alias;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::maintenance::MaintenanceSettings;
use whl_store::{
    AliasAvailability, AliasClaim, EndpointSettings, Overflow, PutAliasOutcome,
    ReleaseAliasOutcome, Store,
};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const QUARANTINE: Duration = Duration::from_secs(30 * 86_400);
const MAX_OWNED: u32 = 5;

fn settings() -> EndpointSettings {
    EndpointSettings {
        title: None,
        response: ResponseConfig::default(),
        cors: false,
        overflow: Overflow::Refuse,
        tags: Vec::new(),
        verifier: None,
        templating: Default::default(),
    }
}

async fn insert_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, 'user@example.test') \
         RETURNING id",
    )
    .fetch_one(pool)
    .await
    .expect("the user inserts")
}

async fn endpoint_of(store: &Store, owner: Option<Uuid>) -> EndpointId {
    store
        .create_endpoint_owned(owner, &settings(), RETENTION)
        .await
        .expect("the endpoint is stored")
        .id
}

fn alias(text: &str) -> Alias {
    Alias::parse(text).expect("the alias is valid")
}

async fn put(
    store: &Store,
    endpoint: &EndpointId,
    user: Uuid,
    name: &str,
    max_owned: u32,
) -> PutAliasOutcome {
    let alias = alias(name);
    store
        .put_alias(AliasClaim {
            endpoint,
            user,
            alias: &alias,
            retention: RETENTION,
            quarantine: QUARANTINE,
            max_owned,
        })
        .await
        .expect("the store answers")
}

fn stored(outcome: &PutAliasOutcome) -> bool {
    matches!(outcome, PutAliasOutcome::Stored(_))
}

async fn age_release(pool: &PgPool, name: &str, seconds: i64) {
    sqlx::query(
        "UPDATE endpoint_aliases SET released_at = now() - make_interval(secs => $2) WHERE alias = $1",
    )
    .bind(name)
    .bind(seconds as f64)
    .execute(pool)
    .await
    .expect("the release time moves");
}

#[tokio::test]
async fn a_stored_alias_resolves_to_its_endpoint_and_is_listed() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;
    let endpoint = endpoint_of(&store, Some(user)).await;

    assert!(stored(
        &put(&store, &endpoint, user, "my-hook", MAX_OWNED).await
    ));

    assert_eq!(
        store
            .resolve_alias(&alias("my-hook"))
            .await
            .expect("resolves"),
        Some(endpoint.clone())
    );
    assert_eq!(
        store
            .resolve_alias(&alias("other-hook"))
            .await
            .expect("resolves"),
        None
    );
    let listed = store.list_aliases(&endpoint).await.expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].alias.unpack(), "my-hook");
}

#[tokio::test]
async fn putting_the_same_alias_again_is_idempotent_and_another_endpoint_cannot_take_it() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;
    let first = endpoint_of(&store, Some(user)).await;
    let second = endpoint_of(&store, Some(user)).await;

    let created = put(&store, &first, user, "shared", MAX_OWNED).await;
    let again = put(&store, &first, user, "shared", MAX_OWNED).await;
    assert_eq!(created, again);
    assert_eq!(
        put(&store, &second, user, "shared", MAX_OWNED).await,
        PutAliasOutcome::Taken
    );
}

#[tokio::test]
async fn the_alias_count_of_a_user_is_capped_across_their_endpoints() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;
    let first = endpoint_of(&store, Some(user)).await;
    let second = endpoint_of(&store, Some(user)).await;

    assert!(stored(&put(&store, &first, user, "one", 2).await));
    assert!(stored(&put(&store, &second, user, "two", 2).await));
    assert_eq!(
        put(&store, &second, user, "three", 2).await,
        PutAliasOutcome::LimitReached
    );
    assert!(stored(&put(&store, &first, user, "one", 2).await));
}

#[tokio::test]
async fn only_the_owner_of_a_live_endpoint_can_alias_it() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let stranger = insert_user(db.pool()).await;
    let owned = endpoint_of(&store, Some(owner)).await;
    let ownerless = endpoint_of(&store, None).await;
    let unknown = EndpointId::parse("000000000000000000000000").expect("a valid id");

    assert_eq!(
        put(&store, &owned, stranger, "stolen", MAX_OWNED).await,
        PutAliasOutcome::NotOwner
    );
    assert_eq!(
        put(&store, &ownerless, owner, "nobody", MAX_OWNED).await,
        PutAliasOutcome::NotOwner
    );
    assert_eq!(
        put(&store, &unknown, owner, "missing", MAX_OWNED).await,
        PutAliasOutcome::EndpointGone
    );
}

#[tokio::test]
async fn a_released_alias_is_closed_to_other_users_during_quarantine() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let other = insert_user(db.pool()).await;
    let owner_endpoint = endpoint_of(&store, Some(owner)).await;
    let other_endpoint = endpoint_of(&store, Some(other)).await;
    let name = alias("quarantined");

    assert!(stored(
        &put(&store, &owner_endpoint, owner, "quarantined", MAX_OWNED).await
    ));
    assert_eq!(
        store
            .release_alias(&owner_endpoint, &name)
            .await
            .expect("releases"),
        ReleaseAliasOutcome::Released
    );
    assert_eq!(store.resolve_alias(&name).await.expect("resolves"), None);

    assert_eq!(
        put(&store, &other_endpoint, other, "quarantined", MAX_OWNED).await,
        PutAliasOutcome::Taken
    );
    assert_eq!(
        store
            .alias_availability(&name, other, QUARANTINE)
            .await
            .expect("answers"),
        AliasAvailability::Taken
    );
}

#[tokio::test]
async fn the_previous_owner_takes_a_released_alias_back_at_once() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let first = endpoint_of(&store, Some(owner)).await;
    let second = endpoint_of(&store, Some(owner)).await;
    let name = alias("moving");

    assert!(stored(
        &put(&store, &first, owner, "moving", MAX_OWNED).await
    ));
    store.release_alias(&first, &name).await.expect("releases");

    assert_eq!(
        store
            .alias_availability(&name, owner, QUARANTINE)
            .await
            .expect("answers"),
        AliasAvailability::Available
    );
    assert!(stored(
        &put(&store, &second, owner, "moving", MAX_OWNED).await
    ));
    assert_eq!(
        store.resolve_alias(&name).await.expect("resolves"),
        Some(second)
    );
}

#[tokio::test]
async fn another_user_takes_a_released_alias_once_the_quarantine_has_passed() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let other = insert_user(db.pool()).await;
    let owner_endpoint = endpoint_of(&store, Some(owner)).await;
    let other_endpoint = endpoint_of(&store, Some(other)).await;
    let name = alias("aged-out");

    assert!(stored(
        &put(&store, &owner_endpoint, owner, "aged-out", MAX_OWNED).await
    ));
    store
        .release_alias(&owner_endpoint, &name)
        .await
        .expect("releases");
    let quarantine_seconds = i64::try_from(QUARANTINE.as_secs()).expect("fits");
    age_release(db.pool(), "aged-out", quarantine_seconds + 60).await;

    assert!(stored(
        &put(&store, &other_endpoint, other, "aged-out", MAX_OWNED).await
    ));
    assert_eq!(
        store.resolve_alias(&name).await.expect("resolves"),
        Some(other_endpoint)
    );
}

#[tokio::test]
async fn deleting_an_endpoint_releases_its_aliases_into_quarantine() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let other = insert_user(db.pool()).await;
    let doomed = endpoint_of(&store, Some(owner)).await;
    let purged = endpoint_of(&store, Some(owner)).await;
    let other_endpoint = endpoint_of(&store, Some(other)).await;

    assert!(stored(
        &put(&store, &doomed, owner, "soft-gone", MAX_OWNED).await
    ));
    assert!(stored(
        &put(&store, &purged, owner, "hard-gone", MAX_OWNED).await
    ));
    assert!(
        store
            .delete_endpoint(&doomed, RETENTION)
            .await
            .expect("deletes")
    );
    sqlx::query("DELETE FROM endpoints WHERE id = $1")
        .bind(purged.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint is purged");

    for name in ["soft-gone", "hard-gone"] {
        assert_eq!(
            store.resolve_alias(&alias(name)).await.expect("resolves"),
            None,
            "{name}"
        );
        assert_eq!(
            put(&store, &other_endpoint, other, name, MAX_OWNED).await,
            PutAliasOutcome::Taken,
            "{name}"
        );
    }
}

#[tokio::test]
async fn releasing_an_alias_of_another_endpoint_finds_nothing() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = insert_user(db.pool()).await;
    let first = endpoint_of(&store, Some(owner)).await;
    let second = endpoint_of(&store, Some(owner)).await;

    assert!(stored(&put(&store, &first, owner, "mine", MAX_OWNED).await));
    assert_eq!(
        store
            .release_alias(&second, &alias("mine"))
            .await
            .expect("answers"),
        ReleaseAliasOutcome::NotFound
    );
    assert_eq!(
        store.resolve_alias(&alias("mine")).await.expect("resolves"),
        Some(first)
    );
}

#[tokio::test]
async fn two_users_racing_for_a_free_alias_leave_exactly_one_winner() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let first_user = insert_user(db.pool()).await;
    let second_user = insert_user(db.pool()).await;
    let first = endpoint_of(&store, Some(first_user)).await;
    let second = endpoint_of(&store, Some(second_user)).await;

    let (a, b) = tokio::join!(
        put(&store, &first, first_user, "contested", MAX_OWNED),
        put(&store, &second, second_user, "contested", MAX_OWNED)
    );

    assert_eq!(
        [&a, &b]
            .into_iter()
            .filter(|outcome| stored(outcome))
            .count(),
        1
    );
    assert!([&a, &b].contains(&&PutAliasOutcome::Taken));
}

#[tokio::test]
async fn an_unknown_alias_is_available() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;

    assert_eq!(
        store
            .alias_availability(&alias("fresh-name"), user, QUARANTINE)
            .await
            .expect("answers"),
        AliasAvailability::Available
    );
}

#[tokio::test]
async fn released_names_in_quarantine_still_count_toward_the_limit() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;
    let endpoint = endpoint_of(&store, Some(user)).await;

    assert!(stored(&put(&store, &endpoint, user, "one", 2).await));
    assert!(stored(&put(&store, &endpoint, user, "two", 2).await));
    store
        .release_alias(&endpoint, &alias("one"))
        .await
        .expect("releases");

    assert_eq!(
        put(&store, &endpoint, user, "three", 2).await,
        PutAliasOutcome::LimitReached
    );
    assert!(stored(&put(&store, &endpoint, user, "one", 2).await));

    store
        .release_alias(&endpoint, &alias("one"))
        .await
        .expect("releases again");
    let quarantine_seconds = i64::try_from(QUARANTINE.as_secs()).expect("fits");
    age_release(db.pool(), "one", quarantine_seconds + 60).await;
    assert!(stored(&put(&store, &endpoint, user, "three", 2).await));
}

#[tokio::test]
async fn the_purge_deletes_released_aliases_past_their_quarantine_in_bounded_batches() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool()).await;
    let endpoint = endpoint_of(&store, Some(user)).await;
    let quarantine_seconds = i64::try_from(QUARANTINE.as_secs()).expect("fits");
    for name in ["old-one", "old-two", "old-three"] {
        sqlx::query(
            "INSERT INTO endpoint_aliases (alias, owner_id, released_at) \
             VALUES ($1, $2, now() - make_interval(secs => $3))",
        )
        .bind(name)
        .bind(user)
        .bind((quarantine_seconds + 60) as f64)
        .execute(db.pool())
        .await
        .expect("the released alias inserts");
    }
    sqlx::query(
        "INSERT INTO endpoint_aliases (alias, owner_id, released_at) VALUES ('recent-one', $1, now())",
    )
    .bind(user)
    .execute(db.pool())
    .await
    .expect("the recent alias inserts");
    assert!(stored(
        &put(&store, &endpoint, user, "active-one", MAX_OWNED).await
    ));
    let settings = MaintenanceSettings {
        alias_quarantine: QUARANTINE,
        alias_purge_batch: 2,
        ..MaintenanceSettings::new(RETENTION, u64::MAX)
    };

    assert_eq!(
        store
            .purge_released_aliases(&settings)
            .await
            .expect("purges"),
        2
    );
    assert_eq!(
        store
            .purge_released_aliases(&settings)
            .await
            .expect("purges"),
        1
    );
    assert_eq!(
        store
            .purge_released_aliases(&settings)
            .await
            .expect("purges"),
        0
    );

    let left: Vec<String> = sqlx::query_scalar("SELECT alias FROM endpoint_aliases ORDER BY alias")
        .fetch_all(db.pool())
        .await
        .expect("the rows read");
    assert_eq!(left, ["active-one", "recent-one"]);
}

#[tokio::test]
async fn a_closed_pool_makes_the_alias_lookup_unavailable_at_once() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    db.pool().close().await;

    let error = store
        .resolve_alias(&alias("any-name"))
        .await
        .expect_err("the pool is closed");

    assert!(error.is_unavailable(), "{error}");
}
