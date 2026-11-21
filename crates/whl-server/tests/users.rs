use sqlx::PgPool;
use whl_core::tier::{TierLimits, Tiers};
use whl_server::{SetTierError, set_user_tier};
use whl_store::{Store, Uuid};
use whl_testkit::TestDb;

fn tiers() -> Tiers {
    Tiers::default()
        .with_limits("pro", TierLimits::member())
        .expect("the tier is valid")
}

async fn insert_user(pool: &PgPool, email: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (id, issuer, subject, email, email_verified) \
         VALUES (gen_random_uuid(), 'issuer', gen_random_uuid()::text, $1, true) RETURNING id",
    )
    .bind(email)
    .fetch_one(pool)
    .await
    .expect("the user inserts")
}

async fn tier_of(pool: &PgPool, user: Uuid) -> String {
    sqlx::query_scalar("SELECT tier FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .expect("the tier reads")
}

#[tokio::test]
async fn a_configured_tier_is_assigned_to_the_user_named_by_email() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool(), "person@example.test").await;

    let change = set_user_tier(&store, &tiers(), "person@example.test", "pro")
        .await
        .expect("the tier is set");

    assert_eq!(change.user, user);
    assert_eq!(change.previous, "member");
    assert_eq!(tier_of(db.pool(), user).await, "pro");
}

#[tokio::test]
async fn a_tier_the_instance_does_not_define_or_cannot_assign_is_refused() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let user = insert_user(db.pool(), "person@example.test").await;

    let unknown = set_user_tier(&store, &tiers(), "person@example.test", "gold").await;
    let anonymous = set_user_tier(&store, &tiers(), "person@example.test", "anonymous").await;

    assert!(matches!(unknown, Err(SetTierError::UnknownTier(_))));
    assert!(matches!(anonymous, Err(SetTierError::UnknownTier(_))));
    assert_eq!(tier_of(db.pool(), user).await, "member");
}

#[tokio::test]
async fn an_ambiguous_email_is_refused_with_the_ids_to_choose_from() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let first = insert_user(db.pool(), "shared@example.test").await;
    insert_user(db.pool(), "shared@example.test").await;

    let refused = set_user_tier(&store, &tiers(), "shared@example.test", "pro").await;
    let by_id = set_user_tier(&store, &tiers(), &first.hyphenated().to_string(), "pro").await;

    match refused {
        Err(SetTierError::Ambiguous { count, ids }) => {
            assert_eq!(count, 2);
            assert!(ids.contains(&first.hyphenated().to_string()));
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
    assert_eq!(by_id.expect("the id names one user").user, first);
}

#[tokio::test]
async fn an_unknown_user_is_reported() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());

    let outcome = set_user_tier(&store, &tiers(), "nobody@example.test", "pro").await;

    assert!(matches!(outcome, Err(SetTierError::NoSuchUser(_))));
}
