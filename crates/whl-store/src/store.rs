use sea_orm::{DatabaseConnection, SqlxPostgresConnector};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use whl_core::config::DatabaseConfig;

use crate::error::StoreError;

static MIGRATIONS: migrator::Migrations = migrator::embed!("$CARGO_MANIFEST_DIR/migrations");

#[derive(Clone, Debug)]
pub struct Store {
    pool: PgPool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationState {
    pub name: String,
    pub applied: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub migrations: Vec<MigrationState>,
    pub unknown: Vec<String>,
}

impl Store {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.pool_size)
            .connect(config.url.unpack().as_str())
            .await
            .map_err(|source| StoreError::Connect { source })?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn migrate_up(&self) -> Result<Vec<String>, StoreError> {
        Ok(MIGRATIONS.apply(&self.database()).await?)
    }

    pub async fn migrate_down(&self) -> Result<Option<String>, StoreError> {
        Ok(MIGRATIONS.revert(&self.database(), 1).await?.pop())
    }

    pub async fn migration_status(&self) -> Result<Status, StoreError> {
        let status = MIGRATIONS.status(&self.database()).await?;
        let migrations = status
            .entries
            .into_iter()
            .map(|entry| MigrationState {
                name: entry.name,
                applied: entry.applied,
            })
            .collect();
        Ok(Status {
            migrations,
            unknown: status.unknown,
        })
    }

    pub async fn check_schema_version(&self) -> Result<(), StoreError> {
        let missing: Vec<String> = self
            .migration_status()
            .await?
            .migrations
            .into_iter()
            .filter(|migration| !migration.applied)
            .map(|migration| migration.name)
            .collect();
        if missing.is_empty() {
            tracing::info!("database schema matches this build");
            return Ok(());
        }
        Err(StoreError::SchemaBehind { missing })
    }

    fn database(&self) -> DatabaseConnection {
        SqlxPostgresConnector::from_sqlx_postgres_pool(self.pool.clone())
    }
}
