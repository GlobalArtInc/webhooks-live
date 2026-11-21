use std::time::Duration;

use whl_store::{
    CreateTokenOutcome, NewApiToken, RevokeTokenOutcome, Store, TokenCaps, TokenScope, Uuid,
    VerifiedIdentity,
};
use whl_testkit::TestDb;

const LIFETIME: Duration = Duration::from_secs(3600);
const CAPS: TokenCaps = TokenCaps {
    max_active: 3,
    max_per_hour: 10,
};

async fn user(store: &Store, tag: &str) -> Uuid {
    store
        .upsert_user(&VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: format!("subject-{tag}"),
            email: format!("{tag}@example.test"),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored")
        .id
}

fn hash(byte: u8) -> Vec<u8> {
    vec![byte; 32]
}

fn prefix(tag: &str) -> String {
    format!("whl_{tag:0>8}")
}

async fn create(
    store: &Store,
    user: Uuid,
    tag: &str,
    byte: u8,
    caps: TokenCaps,
) -> CreateTokenOutcome {
    store
        .create_api_token(
            user,
            &NewApiToken {
                name: tag,
                prefix: &prefix(tag),
                token_hash: &hash(byte),
                scopes: &[TokenScope::Read],
                endpoint_ids: None,
                lifetime: LIFETIME,
            },
            caps,
        )
        .await
        .expect("the query runs")
}

#[tokio::test]
async fn the_active_count_cap_ignores_revoked_and_expired_tokens() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let mut ids = Vec::new();
    for index in 0..3_u8 {
        let CreateTokenOutcome::Created(record) =
            create(&store, owner, &format!("t{index}"), index, CAPS).await
        else {
            panic!("token {index} is created");
        };
        ids.push(record.id);
    }
    assert_eq!(
        create(&store, owner, "t3", 3, CAPS).await,
        CreateTokenOutcome::TooMany
    );

    assert_eq!(
        store
            .revoke_api_token(owner, ids[0])
            .await
            .expect("revokes"),
        RevokeTokenOutcome::Revoked
    );
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(ids[1])
        .execute(db.pool())
        .await
        .expect("the update runs");
    assert!(matches!(
        create(&store, owner, "t4", 4, CAPS).await,
        CreateTokenOutcome::Created(_)
    ));
    assert!(matches!(
        create(&store, owner, "t5", 5, CAPS).await,
        CreateTokenOutcome::Created(_)
    ));
    assert_eq!(
        create(&store, owner, "t6", 6, CAPS).await,
        CreateTokenOutcome::TooMany
    );
}

#[tokio::test]
async fn the_hourly_creation_cap_counts_revoked_tokens_and_reports_the_reset() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let other = user(&store, "oth").await;
    let caps = TokenCaps {
        max_active: 50,
        max_per_hour: 2,
    };
    for index in 0..2_u8 {
        let CreateTokenOutcome::Created(record) =
            create(&store, owner, &format!("t{index}"), index, caps).await
        else {
            panic!("token {index} is created");
        };
        store
            .revoke_api_token(owner, record.id)
            .await
            .expect("revokes");
    }
    let CreateTokenOutcome::TooFast { resets_at } = create(&store, owner, "t2", 2, caps).await
    else {
        panic!("the third creation in the hour is refused");
    };
    let wait = resets_at - time::OffsetDateTime::now_utc();
    assert!(wait.whole_seconds() > 3500 && wait.whole_seconds() <= 3600);
    assert!(matches!(
        create(&store, other, "t3", 3, caps).await,
        CreateTokenOutcome::Created(_)
    ));
}

#[tokio::test]
async fn revoking_is_idempotent_and_scoped_to_the_owner() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let other = user(&store, "oth").await;
    let CreateTokenOutcome::Created(record) = create(&store, owner, "t0", 0, CAPS).await else {
        panic!("the token is created");
    };
    assert_eq!(
        store
            .revoke_api_token(other, record.id)
            .await
            .expect("runs"),
        RevokeTokenOutcome::NotFound
    );
    assert_eq!(
        store
            .revoke_api_token(owner, record.id)
            .await
            .expect("runs"),
        RevokeTokenOutcome::Revoked
    );
    let first = store.list_api_tokens(owner).await.expect("lists")[0].revoked_at;
    assert_eq!(
        store
            .revoke_api_token(owner, record.id)
            .await
            .expect("runs"),
        RevokeTokenOutcome::Revoked
    );
    assert_eq!(
        store.list_api_tokens(owner).await.expect("lists")[0].revoked_at,
        first
    );
    assert_eq!(
        store
            .revoke_api_token(owner, Uuid::nil())
            .await
            .expect("runs"),
        RevokeTokenOutcome::NotFound
    );
}

#[tokio::test]
async fn lookup_reports_state_and_a_touch_is_written_at_most_once_per_interval() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let CreateTokenOutcome::Created(record) = create(&store, owner, "t0", 7, CAPS).await else {
        panic!("the token is created");
    };
    let interval = Duration::from_secs(60);
    let rows = store
        .lookup_api_tokens(&prefix("t0"), interval)
        .await
        .expect("lookup runs");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].token_hash, hash(7));
    assert!(rows[0].touch_due && !rows[0].expired && !rows[0].revoked);
    assert!(
        store
            .lookup_api_tokens("whl_nothing0", interval)
            .await
            .expect("runs")
            .is_empty()
    );

    assert!(
        store
            .touch_api_token(record.id, interval)
            .await
            .expect("touches")
    );
    assert!(
        !store
            .touch_api_token(record.id, interval)
            .await
            .expect("touches")
    );
    let rows = store
        .lookup_api_tokens(&prefix("t0"), interval)
        .await
        .expect("lookup runs");
    assert!(!rows[0].touch_due);

    store
        .revoke_api_token(owner, record.id)
        .await
        .expect("revokes");
    let rows = store
        .lookup_api_tokens(&prefix("t0"), interval)
        .await
        .expect("lookup runs");
    assert!(rows[0].revoked);
    assert!(
        !store
            .touch_api_token(record.id, Duration::ZERO)
            .await
            .expect("runs")
    );
}

#[tokio::test]
async fn stale_tokens_are_deleted_thirty_days_after_expiry_or_revocation() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let mut ids = Vec::new();
    for index in 0..4_u8 {
        let CreateTokenOutcome::Created(record) = create(
            &store,
            owner,
            &format!("t{index}"),
            index,
            TokenCaps {
                max_active: 50,
                max_per_hour: 50,
            },
        )
        .await
        else {
            panic!("token {index} is created");
        };
        ids.push(record.id);
    }
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '31 days' WHERE id = $1")
        .bind(ids[0])
        .execute(db.pool())
        .await
        .expect("the update runs");
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '29 days' WHERE id = $1")
        .bind(ids[1])
        .execute(db.pool())
        .await
        .expect("the update runs");
    sqlx::query("UPDATE api_tokens SET revoked_at = now() - interval '31 days' WHERE id = $1")
        .bind(ids[2])
        .execute(db.pool())
        .await
        .expect("the update runs");
    let grace = Duration::from_secs(30 * 24 * 3600);
    assert_eq!(
        store.delete_stale_api_tokens(grace).await.expect("deletes"),
        2
    );
    let mut left: Vec<Uuid> = store
        .list_api_tokens(owner)
        .await
        .expect("lists")
        .into_iter()
        .map(|record| record.id)
        .collect();
    left.sort();
    let mut expected = vec![ids[1], ids[3]];
    expected.sort();
    assert_eq!(left, expected);
}

#[tokio::test]
async fn two_parallel_creations_at_nineteen_active_tokens_let_exactly_one_through() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let caps = TokenCaps {
        max_active: 20,
        max_per_hour: 100,
    };
    for index in 0..19_u8 {
        assert!(matches!(
            create(&store, owner, &format!("t{index}"), index, caps).await,
            CreateTokenOutcome::Created(_)
        ));
    }
    let (first, second) = tokio::join!(
        create(&store, owner, "pa", 100, caps),
        create(&store, owner, "pb", 101, caps)
    );
    let created = [&first, &second]
        .iter()
        .filter(|outcome| matches!(outcome, CreateTokenOutcome::Created(_)))
        .count();
    let refused = [&first, &second]
        .iter()
        .filter(|outcome| matches!(outcome, CreateTokenOutcome::TooMany))
        .count();
    assert_eq!((created, refused), (1, 1));
}

#[tokio::test]
async fn the_listing_puts_active_tokens_before_revoked_and_expired_ones() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = user(&store, "own").await;
    let caps = TokenCaps {
        max_active: 50,
        max_per_hour: 50,
    };
    let mut ids = Vec::new();
    for index in 0..3_u8 {
        let CreateTokenOutcome::Created(record) =
            create(&store, owner, &format!("t{index}"), index, caps).await
        else {
            panic!("token {index} is created");
        };
        ids.push(record.id);
    }
    store
        .revoke_api_token(owner, ids[2])
        .await
        .expect("revokes");
    sqlx::query("UPDATE api_tokens SET expires_at = now() - interval '1 minute' WHERE id = $1")
        .bind(ids[1])
        .execute(db.pool())
        .await
        .expect("the update runs");
    let listed: Vec<Uuid> = store
        .list_api_tokens(owner)
        .await
        .expect("lists")
        .into_iter()
        .map(|record| record.id)
        .collect();
    assert_eq!(listed, [ids[0], ids[2], ids[1]]);
}
