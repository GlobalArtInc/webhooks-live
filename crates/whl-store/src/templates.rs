use serde_json::Value;
use sqlx::Row;
use sqlx::postgres::PgRow;
use sqlx::types::Uuid;
use time::OffsetDateTime;

use crate::store::Store;

pub const MAX_TEMPLATES_PER_USER: usize = 20;

const UNIQUE_VIOLATION: &str = "23505";

const TEMPLATE_COLUMNS: &str = "id, name, config::text AS config, created_at, updated_at";

#[derive(Debug, Clone, PartialEq)]
pub struct TemplateRecord {
    pub id: Uuid,
    pub name: String,
    pub config: Value,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSummary {
    pub id: Uuid,
    pub name: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CreateTemplateOutcome {
    Created(TemplateRecord),
    NameTaken,
    LimitReached,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenameTemplateOutcome {
    Renamed(TemplateRecord),
    NameTaken,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("template query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored template {id} is invalid: {reason}")]
    Invalid { id: Uuid, reason: String },
    #[error("cannot encode the template configuration: {0}")]
    Encode(#[source] serde_json::Error),
}

impl TemplateError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl Store {
    pub async fn list_templates(&self, user: Uuid) -> Result<Vec<TemplateSummary>, TemplateError> {
        let rows = sqlx::query(
            "SELECT id, name, created_at, updated_at FROM endpoint_templates \
             WHERE user_id = $1 ORDER BY created_at, id",
        )
        .bind(user)
        .fetch_all(self.pool())
        .await
        .map_err(classify)?;
        rows.iter()
            .map(|row| {
                Ok(TemplateSummary {
                    id: row.try_get("id").map_err(TemplateError::Query)?,
                    name: row.try_get("name").map_err(TemplateError::Query)?,
                    created_at: row.try_get("created_at").map_err(TemplateError::Query)?,
                    updated_at: row.try_get("updated_at").map_err(TemplateError::Query)?,
                })
            })
            .collect()
    }

    pub async fn get_template(
        &self,
        user: Uuid,
        id: Uuid,
    ) -> Result<Option<TemplateRecord>, TemplateError> {
        let sql = format!(
            "SELECT {TEMPLATE_COLUMNS} FROM endpoint_templates WHERE user_id = $1 AND id = $2"
        );
        let row = sqlx::query(&sql)
            .bind(user)
            .bind(id)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(template_from_row).transpose()
    }

    pub async fn create_template(
        &self,
        user: Uuid,
        name: &str,
        config: &Value,
    ) -> Result<CreateTemplateOutcome, TemplateError> {
        let encoded = serde_json::to_string(config).map_err(TemplateError::Encode)?;
        let mut tx = self.pool().begin().await.map_err(classify)?;
        sqlx::query("SELECT id FROM users WHERE id = $1 FOR UPDATE")
            .bind(user)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let held: i64 =
            sqlx::query_scalar("SELECT count(*) FROM endpoint_templates WHERE user_id = $1")
                .bind(user)
                .fetch_one(&mut *tx)
                .await
                .map_err(classify)?;
        if usize::try_from(held).map_or(true, |held| held >= MAX_TEMPLATES_PER_USER) {
            return Ok(CreateTemplateOutcome::LimitReached);
        }
        let sql = format!(
            "INSERT INTO endpoint_templates (id, user_id, name, config) \
             VALUES ($1, $2, $3, $4::text::jsonb) RETURNING {TEMPLATE_COLUMNS}"
        );
        let inserted = sqlx::query(&sql)
            .bind(Uuid::now_v7())
            .bind(user)
            .bind(name)
            .bind(encoded)
            .fetch_one(&mut *tx)
            .await;
        let row = match inserted {
            Ok(row) => row,
            Err(error) if is_unique_violation(&error) => {
                return Ok(CreateTemplateOutcome::NameTaken);
            }
            Err(error) => return Err(classify(error)),
        };
        let record = template_from_row(&row)?;
        tx.commit().await.map_err(classify)?;
        Ok(CreateTemplateOutcome::Created(record))
    }

    pub async fn rename_template(
        &self,
        user: Uuid,
        id: Uuid,
        name: &str,
    ) -> Result<RenameTemplateOutcome, TemplateError> {
        let sql = format!(
            "UPDATE endpoint_templates SET name = $3, updated_at = now() \
             WHERE user_id = $1 AND id = $2 RETURNING {TEMPLATE_COLUMNS}"
        );
        let updated = sqlx::query(&sql)
            .bind(user)
            .bind(id)
            .bind(name)
            .fetch_optional(self.pool())
            .await;
        match updated {
            Ok(Some(row)) => Ok(RenameTemplateOutcome::Renamed(template_from_row(&row)?)),
            Ok(None) => Ok(RenameTemplateOutcome::NotFound),
            Err(error) if is_unique_violation(&error) => Ok(RenameTemplateOutcome::NameTaken),
            Err(error) => Err(classify(error)),
        }
    }

    pub async fn delete_template(&self, user: Uuid, id: Uuid) -> Result<bool, TemplateError> {
        let result = sqlx::query("DELETE FROM endpoint_templates WHERE user_id = $1 AND id = $2")
            .bind(user)
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(result.rows_affected() == 1)
    }
}

fn template_from_row(row: &PgRow) -> Result<TemplateRecord, TemplateError> {
    let id: Uuid = row.try_get("id").map_err(TemplateError::Query)?;
    let config: String = row.try_get("config").map_err(TemplateError::Query)?;
    let config = serde_json::from_str(&config).map_err(|error| TemplateError::Invalid {
        id,
        reason: error.to_string(),
    })?;
    Ok(TemplateRecord {
        id,
        name: row.try_get("name").map_err(TemplateError::Query)?,
        config,
        created_at: row.try_get("created_at").map_err(TemplateError::Query)?,
        updated_at: row.try_get("updated_at").map_err(TemplateError::Query)?,
    })
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some(UNIQUE_VIOLATION)
    )
}

fn classify(error: sqlx::Error) -> TemplateError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        TemplateError::Unavailable(error)
    } else {
        TemplateError::Query(error)
    }
}
