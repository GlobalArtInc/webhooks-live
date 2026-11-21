use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::postgres::{PgConnection, PgPoolOptions};
use sqlx::{Connection, PgPool};
use url::Url;
use whl_store::Store;

const ADMIN_URL_VAR: &str = "WEBHOOKS_LIVE_TEST_DATABASE_URL";
const POOL_SIZE: u32 = 4;

static NEXT: AtomicU32 = AtomicU32::new(0);

pub struct TestDb {
    admin_url: String,
    name: String,
    url: String,
    pool: PgPool,
}

impl TestDb {
    pub async fn new() -> Self {
        let admin_url = admin_url();
        let name = unique_name();

        let mut admin = PgConnection::connect(&admin_url)
            .await
            .unwrap_or_else(|error| panic!("cannot reach the test server: {error}"));
        sqlx::raw_sql(&format!("CREATE DATABASE \"{name}\""))
            .execute(&mut admin)
            .await
            .unwrap_or_else(|error| panic!("cannot create the test database {name}: {error}"));
        let _ = admin.close().await;

        let url = database_url(&admin_url, &name);
        let pool = PgPoolOptions::new()
            .max_connections(POOL_SIZE)
            .connect(&url)
            .await
            .unwrap_or_else(|error| panic!("cannot connect to the test database {name}: {error}"));

        let db = Self {
            admin_url,
            name,
            url,
            pool,
        };
        Store::from_pool(db.pool.clone())
            .migrate_up()
            .await
            .unwrap_or_else(|error| {
                panic!("cannot migrate the test database {}: {error}", db.name)
            });
        db
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        let worker = name.clone();
        let dropped = std::thread::spawn(move || drop_database(&admin_url, &worker)).join();
        if !matches!(dropped, Ok(true)) {
            eprintln!("the test database {name} was left behind");
        }
    }
}

fn drop_database(admin_url: &str, name: &str) -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        let Ok(mut admin) = PgConnection::connect(admin_url).await else {
            return false;
        };
        let dropped = sqlx::raw_sql(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
            .execute(&mut admin)
            .await
            .is_ok();
        let _ = admin.close().await;
        dropped
    })
}

fn admin_url() -> String {
    std::env::var(ADMIN_URL_VAR).unwrap_or_else(|_| {
        panic!(
            "set {ADMIN_URL_VAR} to a PostgreSQL URL whose user may create databases; \
             start the server with tools/test-db.sh up, then run: export $(tools/test-db.sh url)"
        )
    })
}

fn database_url(admin_url: &str, name: &str) -> String {
    let mut url = Url::parse(admin_url)
        .unwrap_or_else(|error| panic!("{ADMIN_URL_VAR} is not a valid URL: {error}"));
    url.set_path(&format!("/{name}"));
    url.into()
}

fn unique_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or_default();
    format!(
        "whl_test_{}_{}_{nanos}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
