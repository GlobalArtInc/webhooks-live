use sqlx::PgPool;
use whl_store::{Store, StoreError};
use whl_testkit::TestDb;

const TABLES: [&str; 10] = [
    "capture_bodies",
    "captures",
    "endpoint_members",
    "endpoints",
    "forward_attempts",
    "forward_jobs",
    "forward_rules",
    "login_states",
    "sessions",
    "users",
];

const KEY_INDEXES: [&str; 7] = [
    "users_email",
    "sessions_expires",
    "endpoints_owner",
    "endpoints_deleted",
    "endpoint_members_user",
    "forward_jobs_due",
    "forward_jobs_capture",
];

async fn migration_names(store: &Store) -> Vec<String> {
    store
        .migration_status()
        .await
        .expect("status reads the ledger")
        .migrations
        .into_iter()
        .map(|migration| migration.name)
        .collect()
}

async fn leftover_objects(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT relname::text FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = 'public' AND c.relkind IN ('r', 'S', 'v', 'm') \
           AND c.relname NOT LIKE 'migrations%' \
         UNION ALL \
         SELECT typname::text FROM pg_type t \
         JOIN pg_namespace n ON n.oid = t.typnamespace \
         WHERE n.nspname = 'public' AND t.typtype IN ('e', 'd') \
           AND t.typname NOT LIKE 'migrations%'",
    )
    .fetch_all(pool)
    .await
    .expect("the catalogs are readable")
}

#[tokio::test]
async fn every_migration_applies_up_and_takes_back_to_an_empty_schema() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let names = migration_names(&store).await;
    assert!(!names.is_empty());

    for name in names.iter().rev() {
        assert_eq!(
            store.migrate_down().await.expect("a migration takes back"),
            Some(name.clone())
        );
    }
    assert_eq!(
        store
            .migrate_down()
            .await
            .expect("nothing is left to take back"),
        None
    );
    assert!(leftover_objects(db.pool()).await.is_empty());
}

#[tokio::test]
async fn migration_status_lists_applied_then_pending_migrations() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let status = store
        .migration_status()
        .await
        .expect("status reads the ledger");
    assert!(status.migrations.iter().all(|migration| migration.applied));
    assert!(status.unknown.is_empty());
    let last = status
        .migrations
        .last()
        .map(|migration| migration.name.clone())
        .expect("at least one migration is embedded");

    store
        .migrate_down()
        .await
        .expect("the last migration takes back");
    let status = store
        .migration_status()
        .await
        .expect("status reads the ledger");
    let pending: Vec<&str> = status
        .migrations
        .iter()
        .filter(|migration| !migration.applied)
        .map(|migration| migration.name.as_str())
        .collect();
    assert_eq!(pending, [last.as_str()]);
}

#[tokio::test]
async fn schema_check_passes_on_a_fresh_database() {
    let db = TestDb::new().await;
    Store::from_pool(db.pool().clone())
        .check_schema_version()
        .await
        .expect("a migrated database matches this build");
}

#[tokio::test]
async fn schema_check_names_the_migration_whose_record_is_missing() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let last = migration_names(&store)
        .await
        .pop()
        .expect("at least one migration is embedded");
    sqlx::query("DELETE FROM migrations WHERE name = $1")
        .bind(last.as_str())
        .execute(db.pool())
        .await
        .expect("the ledger row is removed");

    match store.check_schema_version().await {
        Err(StoreError::SchemaBehind { missing }) => assert_eq!(missing, [last]),
        other => panic!("expected a missing migration, got {other:?}"),
    }
}

#[tokio::test]
async fn schema_check_accepts_a_database_ahead_of_the_build() {
    let db = TestDb::new().await;
    sqlx::query(r#"INSERT INTO migrations ("timestamp", name) VALUES (1, 'FutureMigration1')"#)
        .execute(db.pool())
        .await
        .expect("the extra ledger row is written");
    let store = Store::from_pool(db.pool().clone());

    store
        .check_schema_version()
        .await
        .expect("an older build may run against a newer schema");
    let status = store
        .migration_status()
        .await
        .expect("status reads the ledger");
    assert_eq!(status.unknown, ["FutureMigration1"]);
}

#[tokio::test]
async fn schema_has_the_tables_and_key_indexes_of_the_architecture() {
    let db = TestDb::new().await;
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name NOT LIKE 'migrations%' \
         ORDER BY table_name::text",
    )
    .fetch_all(db.pool())
    .await
    .expect("information_schema is readable");
    assert_eq!(tables, TABLES);

    let indexes = sqlx::query_scalar::<_, String>(
        "SELECT indexname::text FROM pg_indexes WHERE schemaname = 'public'",
    )
    .fetch_all(db.pool())
    .await
    .expect("pg_indexes is readable");
    for index in KEY_INDEXES {
        assert!(
            indexes.iter().any(|name| name == index),
            "missing index {index}"
        );
    }
}
