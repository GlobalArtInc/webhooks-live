use std::time::Duration;

use sqlx::types::Uuid;
use sqlx::{Row, postgres::PgRow};
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;

use crate::store::Store;

const UNIQUE_VIOLATION: &str = "23505";

const FOLDER_ROWS: &str = "SELECT f.id, f.name, f.position, f.created_at, ARRAY( \
     SELECT fe.endpoint_id FROM folder_endpoints fe \
     JOIN endpoints e ON e.id = fe.endpoint_id \
     WHERE fe.folder_id = f.id AND e.deleted_at IS NULL \
       AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $2))) \
       AND (e.owner_id = $1 OR EXISTS ( \
         SELECT 1 FROM endpoint_members m WHERE m.endpoint_id = e.id AND m.user_id = $1)) \
     ORDER BY fe.added_at, fe.endpoint_id) AS endpoint_ids \
     FROM folders f WHERE f.user_id = $1 AND ($3::uuid IS NULL OR f.id = $3) \
     ORDER BY f.position, f.created_at, f.id";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRecord {
    pub id: Uuid,
    pub name: String,
    pub position: i32,
    pub created_at: OffsetDateTime,
    pub endpoint_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateFolderOutcome {
    Created(FolderRecord),
    NameTaken,
    LimitReached,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateFolderOutcome {
    Updated(FolderRecord),
    NameTaken,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("folder query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored folder is invalid: {reason}")]
    Invalid { reason: String },
}

impl Store {
    pub async fn list_folders(
        &self,
        user: Uuid,
        retention: Duration,
    ) -> Result<Vec<FolderRecord>, FolderError> {
        let rows = sqlx::query(FOLDER_ROWS)
            .bind(user)
            .bind(retention.as_secs_f64())
            .bind(None::<Uuid>)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(folder_from_row).collect()
    }

    pub async fn create_folder(
        &self,
        user: Uuid,
        name: &str,
        max: usize,
    ) -> Result<CreateFolderOutcome, FolderError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM folders WHERE user_id = $1")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        if count >= i64::try_from(max).unwrap_or(i64::MAX) {
            return Ok(CreateFolderOutcome::LimitReached);
        }
        let position: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(position) + 1, 0) FROM folders WHERE user_id = $1",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .map_err(classify)?;
        let inserted = sqlx::query(
            "INSERT INTO folders (id, user_id, name, position) VALUES ($1, $2, $3, $4) \
             RETURNING id, name, position, created_at",
        )
        .bind(Uuid::now_v7())
        .bind(user)
        .bind(name)
        .bind(position)
        .fetch_one(&mut *tx)
        .await;
        let row = match inserted {
            Ok(row) => row,
            Err(error) if is_unique_violation(&error) => {
                return Ok(CreateFolderOutcome::NameTaken);
            }
            Err(error) => return Err(classify(error)),
        };
        tx.commit().await.map_err(classify)?;
        Ok(CreateFolderOutcome::Created(FolderRecord {
            id: row.try_get("id").map_err(FolderError::Query)?,
            name: row.try_get("name").map_err(FolderError::Query)?,
            position: row.try_get("position").map_err(FolderError::Query)?,
            created_at: row.try_get("created_at").map_err(FolderError::Query)?,
            endpoint_ids: Vec::new(),
        }))
    }

    pub async fn update_folder(
        &self,
        user: Uuid,
        folder: Uuid,
        name: Option<&str>,
        position: Option<i32>,
        retention: Duration,
    ) -> Result<UpdateFolderOutcome, FolderError> {
        let updated = sqlx::query(
            "UPDATE folders SET name = COALESCE($3::text, name), \
             position = COALESCE($4::integer, position) \
             WHERE id = $1 AND user_id = $2 RETURNING id",
        )
        .bind(folder)
        .bind(user)
        .bind(name)
        .bind(position)
        .fetch_optional(self.pool())
        .await;
        match updated {
            Ok(None) => return Ok(UpdateFolderOutcome::NotFound),
            Ok(Some(_)) => {}
            Err(error) if is_unique_violation(&error) => {
                return Ok(UpdateFolderOutcome::NameTaken);
            }
            Err(error) => return Err(classify(error)),
        }
        let rows = sqlx::query(FOLDER_ROWS)
            .bind(user)
            .bind(retention.as_secs_f64())
            .bind(Some(folder))
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let record = rows
            .first()
            .map(folder_from_row)
            .transpose()?
            .ok_or_else(|| FolderError::Invalid {
                reason: format!("folder {folder} vanished after an update"),
            })?;
        Ok(UpdateFolderOutcome::Updated(record))
    }

    pub async fn delete_folder(&self, user: Uuid, folder: Uuid) -> Result<bool, FolderError> {
        let result = sqlx::query("DELETE FROM folders WHERE id = $1 AND user_id = $2")
            .bind(folder)
            .bind(user)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn add_folder_endpoint(
        &self,
        user: Uuid,
        folder: Uuid,
        endpoint: &EndpointId,
    ) -> Result<bool, FolderError> {
        let row = sqlx::query(
            "WITH owned AS (SELECT id FROM folders WHERE id = $1 AND user_id = $3), \
             inserted AS (INSERT INTO folder_endpoints (folder_id, endpoint_id) \
               SELECT id, $2 FROM owned ON CONFLICT DO NOTHING RETURNING folder_id) \
             SELECT EXISTS (SELECT 1 FROM owned) AS owned",
        )
        .bind(folder)
        .bind(endpoint.unpack())
        .bind(user)
        .fetch_one(self.pool())
        .await
        .map_err(classify)?;
        row.try_get("owned").map_err(FolderError::Query)
    }

    pub async fn remove_folder_endpoint(
        &self,
        user: Uuid,
        folder: Uuid,
        endpoint: &EndpointId,
    ) -> Result<bool, FolderError> {
        let row = sqlx::query(
            "WITH owned AS (SELECT id FROM folders WHERE id = $1 AND user_id = $3), \
             removed AS (DELETE FROM folder_endpoints \
               WHERE folder_id IN (SELECT id FROM owned) AND endpoint_id = $2 \
               RETURNING folder_id) \
             SELECT EXISTS (SELECT 1 FROM owned) AS owned",
        )
        .bind(folder)
        .bind(endpoint.unpack())
        .bind(user)
        .fetch_one(self.pool())
        .await
        .map_err(classify)?;
        row.try_get("owned").map_err(FolderError::Query)
    }
}

fn folder_from_row(row: &PgRow) -> Result<FolderRecord, FolderError> {
    Ok(FolderRecord {
        id: row.try_get("id").map_err(FolderError::Query)?,
        name: row.try_get("name").map_err(FolderError::Query)?,
        position: row.try_get("position").map_err(FolderError::Query)?,
        created_at: row.try_get("created_at").map_err(FolderError::Query)?,
        endpoint_ids: row.try_get("endpoint_ids").map_err(FolderError::Query)?,
    })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some(UNIQUE_VIOLATION)
    )
}

fn classify(error: sqlx::Error) -> FolderError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        FolderError::Unavailable(error)
    } else {
        FolderError::Query(error)
    }
}
