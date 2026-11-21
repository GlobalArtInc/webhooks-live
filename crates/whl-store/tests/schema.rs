use sqlx::PgPool;
use whl_store::{Store, StoreError};
use whl_testkit::TestDb;

const TABLES: [&str; 15] = [
    "api_tokens",
    "capture_bodies",
    "captures",
    "endpoint_aliases",
    "endpoint_members",
    "endpoint_secrets",
    "endpoints",
    "folder_endpoints",
    "folders",
    "forward_attempts",
    "forward_jobs",
    "forward_rules",
    "login_states",
    "sessions",
    "users",
];

const KEY_INDEXES: [&str; 12] = [
    "api_tokens_user",
    "api_tokens_prefix",
    "users_email",
    "sessions_expires",
    "endpoints_owner",
    "endpoints_deleted",
    "endpoints_creator_net",
    "endpoint_members_user",
    "forward_jobs_due",
    "forward_jobs_capture",
    "endpoints_tags",
    "folder_endpoints_endpoint",
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

const KIND_MIGRATION: &str = "AddCaptureKinds1795254376505";
const CREATOR_NET_MIGRATION: &str = "AddEndpointCreatorNet1795254378101";
const ENDPOINT_ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

async fn insert_endpoint_with_captures(pool: &PgPool, count: i64) {
    sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
        .bind(ENDPOINT_ID)
        .execute(pool)
        .await
        .expect("the endpoint is inserted");
    for seq in 1..=count {
        sqlx::query(
            "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
             client_ip, headers, query, body_size, stored_bytes, response_status) \
             VALUES ($1, $2, gen_random_uuid(), now(), 'POST', 'https://whl.test/hook', '/hook', \
             'whl.test', '203.0.113.7'::inet, '[]'::jsonb, '[]'::jsonb, 0, 0, 200)",
        )
        .bind(ENDPOINT_ID)
        .bind(seq)
        .execute(pool)
        .await
        .expect("the capture is inserted");
    }
}

#[tokio::test]
async fn existing_captures_read_as_http_with_an_empty_meta_after_a_round_trip() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    loop {
        let reverted = store
            .migrate_down()
            .await
            .expect("a migration takes back")
            .expect("the kinds migration is among the applied ones");
        if reverted == KIND_MIGRATION {
            break;
        }
    }
    insert_endpoint_with_captures(db.pool(), 2).await;

    store
        .migrate_up()
        .await
        .expect("the kinds migration applies over populated captures");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT kind, meta::text FROM captures ORDER BY seq")
            .fetch_all(db.pool())
            .await
            .expect("captures are read");
    assert_eq!(rows, vec![("http".to_owned(), "{}".to_owned()); 2]);
}

#[tokio::test]
async fn the_creator_network_migration_keeps_endpoints_and_leaves_existing_ones_unrecorded() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    loop {
        let reverted = store
            .migrate_down()
            .await
            .expect("a migration takes back")
            .expect("the creator network migration is among the applied ones");
        if reverted == CREATOR_NET_MIGRATION {
            break;
        }
    }
    sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
        .bind(ENDPOINT_ID)
        .execute(db.pool())
        .await
        .expect("the endpoint is inserted");

    store
        .migrate_up()
        .await
        .expect("the creator network migration applies over existing endpoints");
    let net: Option<String> =
        sqlx::query_scalar("SELECT creator_net::text FROM endpoints WHERE id = $1")
            .bind(ENDPOINT_ID)
            .fetch_one(db.pool())
            .await
            .expect("the column exists");
    assert_eq!(net, None);
    sqlx::query("UPDATE endpoints SET creator_net = '203.0.113.0/24' WHERE id = $1")
        .bind(ENDPOINT_ID)
        .execute(db.pool())
        .await
        .expect("a network is stored");
}

#[tokio::test]
async fn the_kind_check_refuses_a_kind_outside_the_allowed_set() {
    let db = TestDb::new().await;
    insert_endpoint_with_captures(db.pool(), 1).await;

    let error = sqlx::query("UPDATE captures SET kind = 'sms' WHERE endpoint_id = $1")
        .bind(ENDPOINT_ID)
        .execute(db.pool())
        .await
        .expect_err("an unknown kind is refused");
    let constraint = error
        .as_database_error()
        .and_then(|database| database.constraint());
    assert_eq!(constraint, Some("captures_kind_check"));
}
