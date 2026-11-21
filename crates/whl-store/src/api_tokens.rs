use std::fmt;
use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::types::Uuid;
use sqlx::{Row, Transaction};
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;

use crate::auth::UserRecord;
use crate::store::Store;

pub const MAX_TOKEN_ENDPOINTS: usize = 50;
pub const MAX_TOKEN_NAME_CHARS: usize = 64;
const LIST_LIMIT: i64 = 500;
const LOOKUP_LIMIT: i64 = 8;
const RECENT_WINDOW: Duration = Duration::from_secs(3600);

const LOCK_USER_SQL: &str = "SELECT id FROM users WHERE id = $1 FOR UPDATE";

const TOKEN_COUNTS_SQL: &str = "SELECT \
        count(*) FILTER (WHERE revoked_at IS NULL AND expires_at > now()) AS active, \
        count(*) FILTER (WHERE created_at > now() - make_interval(secs => $2)) AS recent, \
        min(created_at) FILTER (WHERE created_at > now() - make_interval(secs => $2)) AS oldest \
    FROM api_tokens WHERE user_id = $1";

const INSERT_TOKEN_SQL: &str = "INSERT INTO api_tokens \
        (user_id, name, prefix, token_hash, scopes, endpoint_ids, expires_at) \
    VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(secs => $7)) \
    RETURNING id, name, prefix, scopes, endpoint_ids, created_at, expires_at, last_used_at, revoked_at";

const LIST_TOKENS_SQL: &str = "SELECT id, name, prefix, scopes, endpoint_ids, created_at, \
        expires_at, last_used_at, revoked_at \
    FROM api_tokens WHERE user_id = $1 \
    ORDER BY (revoked_at IS NULL AND expires_at > now()) DESC, created_at DESC, id \
    LIMIT $2";

const REVOKE_TOKEN_SQL: &str = "UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, now()) \
    WHERE id = $1 AND user_id = $2 \
    RETURNING id";

const LOOKUP_TOKENS_SQL: &str = "SELECT t.id, t.token_hash, t.scopes, t.endpoint_ids, \
        t.expires_at <= now() AS expired, t.revoked_at IS NOT NULL AS revoked, \
        (t.last_used_at IS NULL OR t.last_used_at < now() - make_interval(secs => $2)) AS touch_due, \
        u.id AS user_id, u.email, u.display_name \
    FROM api_tokens t JOIN users u ON u.id = t.user_id \
    WHERE t.prefix = $1 \
    ORDER BY t.id \
    LIMIT $3";

const TOUCH_TOKEN_SQL: &str = "UPDATE api_tokens SET last_used_at = now() \
    WHERE id = $1 AND revoked_at IS NULL AND expires_at > now() \
        AND (last_used_at IS NULL OR last_used_at < now() - make_interval(secs => $2))";

const DELETE_STALE_TOKENS_SQL: &str = "DELETE FROM api_tokens \
    WHERE expires_at < now() - make_interval(secs => $1) \
        OR revoked_at < now() - make_interval(secs => $1)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenScope {
    Read,
    EndpointsWrite,
    CapturesWrite,
    RulesWrite,
    Send,
}

impl TokenScope {
    pub const ALL: [Self; 5] = [
        Self::Read,
        Self::EndpointsWrite,
        Self::CapturesWrite,
        Self::RulesWrite,
        Self::Send,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::EndpointsWrite => "endpoints:write",
            Self::CapturesWrite => "captures:write",
            Self::RulesWrite => "rules:write",
            Self::Send => "send",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|scope| scope.as_str() == text)
    }
}

impl fmt::Display for TokenScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiTokenStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("the API token query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored API token data is invalid: {0}")]
    Invalid(String),
}

impl ApiTokenStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

fn classify(error: sqlx::Error) -> ApiTokenStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::WorkerCrashed
    ) {
        ApiTokenStoreError::Unavailable(error)
    } else {
        ApiTokenStoreError::Query(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiTokenRecord {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<TokenScope>,
    pub endpoint_ids: Option<Vec<EndpointId>>,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
    pub last_used_at: Option<OffsetDateTime>,
    pub revoked_at: Option<OffsetDateTime>,
}

#[derive(Clone, Copy)]
pub struct NewApiToken<'a> {
    pub name: &'a str,
    pub prefix: &'a str,
    pub token_hash: &'a [u8],
    pub scopes: &'a [TokenScope],
    pub endpoint_ids: Option<&'a [EndpointId]>,
    pub lifetime: Duration,
}

impl fmt::Debug for NewApiToken<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewApiToken")
            .field("name", &self.name)
            .field("prefix", &self.prefix)
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenCaps {
    pub max_active: u32,
    pub max_per_hour: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateTokenOutcome {
    Created(ApiTokenRecord),
    TooMany,
    TooFast { resets_at: OffsetDateTime },
    UserGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeTokenOutcome {
    Revoked,
    NotFound,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ApiTokenLookup {
    pub id: Uuid,
    pub token_hash: Vec<u8>,
    pub user: UserRecord,
    pub scopes: Vec<TokenScope>,
    pub endpoint_ids: Option<Vec<EndpointId>>,
    pub expired: bool,
    pub revoked: bool,
    pub touch_due: bool,
}

impl fmt::Debug for ApiTokenLookup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiTokenLookup")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("scopes", &self.scopes)
            .field("expired", &self.expired)
            .field("revoked", &self.revoked)
            .finish_non_exhaustive()
    }
}

fn scopes_from(values: &[String]) -> Result<Vec<TokenScope>, ApiTokenStoreError> {
    values
        .iter()
        .map(|text| {
            TokenScope::parse(text)
                .ok_or_else(|| ApiTokenStoreError::Invalid(format!("unknown token scope {text:?}")))
        })
        .collect()
}

fn endpoints_from(
    values: Option<Vec<String>>,
) -> Result<Option<Vec<EndpointId>>, ApiTokenStoreError> {
    values
        .map(|texts| {
            texts
                .iter()
                .map(|text| {
                    EndpointId::parse(text).map_err(|error| {
                        ApiTokenStoreError::Invalid(format!("bad token endpoint id: {error}"))
                    })
                })
                .collect()
        })
        .transpose()
}

fn record_from_row(row: &PgRow) -> Result<ApiTokenRecord, ApiTokenStoreError> {
    let scopes: Vec<String> = row.try_get("scopes").map_err(classify)?;
    let endpoint_ids: Option<Vec<String>> = row.try_get("endpoint_ids").map_err(classify)?;
    Ok(ApiTokenRecord {
        id: row.try_get("id").map_err(classify)?,
        name: row.try_get("name").map_err(classify)?,
        prefix: row.try_get("prefix").map_err(classify)?,
        scopes: scopes_from(&scopes)?,
        endpoint_ids: endpoints_from(endpoint_ids)?,
        created_at: row.try_get("created_at").map_err(classify)?,
        expires_at: row.try_get("expires_at").map_err(classify)?,
        last_used_at: row.try_get("last_used_at").map_err(classify)?,
        revoked_at: row.try_get("revoked_at").map_err(classify)?,
    })
}

fn lookup_from_row(row: &PgRow) -> Result<ApiTokenLookup, ApiTokenStoreError> {
    let scopes: Vec<String> = row.try_get("scopes").map_err(classify)?;
    let endpoint_ids: Option<Vec<String>> = row.try_get("endpoint_ids").map_err(classify)?;
    Ok(ApiTokenLookup {
        id: row.try_get("id").map_err(classify)?,
        token_hash: row.try_get("token_hash").map_err(classify)?,
        user: UserRecord {
            id: row.try_get("user_id").map_err(classify)?,
            email: row.try_get("email").map_err(classify)?,
            display_name: row.try_get("display_name").map_err(classify)?,
        },
        scopes: scopes_from(&scopes)?,
        endpoint_ids: endpoints_from(endpoint_ids)?,
        expired: row.try_get("expired").map_err(classify)?,
        revoked: row.try_get("revoked").map_err(classify)?,
        touch_due: row.try_get("touch_due").map_err(classify)?,
    })
}

async fn insert_token(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    token: &NewApiToken<'_>,
) -> Result<ApiTokenRecord, ApiTokenStoreError> {
    let scopes: Vec<String> = token
        .scopes
        .iter()
        .map(|scope| scope.as_str().to_owned())
        .collect();
    let endpoints: Option<Vec<String>> = token.endpoint_ids.map(|ids| {
        ids.iter()
            .map(|id| id.unpack().to_owned())
            .collect::<Vec<_>>()
    });
    let row = sqlx::query(INSERT_TOKEN_SQL)
        .bind(user_id)
        .bind(token.name)
        .bind(token.prefix)
        .bind(token.token_hash)
        .bind(scopes)
        .bind(endpoints)
        .bind(token.lifetime.as_secs_f64())
        .fetch_one(&mut **tx)
        .await
        .map_err(classify)?;
    record_from_row(&row)
}

impl Store {
    pub async fn create_api_token(
        &self,
        user_id: Uuid,
        token: &NewApiToken<'_>,
        caps: TokenCaps,
    ) -> Result<CreateTokenOutcome, ApiTokenStoreError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let locked: Option<Uuid> = sqlx::query_scalar(LOCK_USER_SQL)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if locked.is_none() {
            return Ok(CreateTokenOutcome::UserGone);
        }
        let counts = sqlx::query(TOKEN_COUNTS_SQL)
            .bind(user_id)
            .bind(RECENT_WINDOW.as_secs_f64())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        let active: i64 = counts.try_get("active").map_err(classify)?;
        let recent: i64 = counts.try_get("recent").map_err(classify)?;
        let oldest: Option<OffsetDateTime> = counts.try_get("oldest").map_err(classify)?;
        if active >= i64::from(caps.max_active) {
            return Ok(CreateTokenOutcome::TooMany);
        }
        if recent >= i64::from(caps.max_per_hour) {
            let resets_at = oldest.map_or_else(
                || OffsetDateTime::now_utc() + RECENT_WINDOW,
                |oldest| oldest + RECENT_WINDOW,
            );
            return Ok(CreateTokenOutcome::TooFast { resets_at });
        }
        let record = insert_token(&mut tx, user_id, token).await?;
        tx.commit().await.map_err(classify)?;
        Ok(CreateTokenOutcome::Created(record))
    }

    pub async fn list_api_tokens(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<ApiTokenRecord>, ApiTokenStoreError> {
        let rows = sqlx::query(LIST_TOKENS_SQL)
            .bind(user_id)
            .bind(LIST_LIMIT)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(record_from_row).collect()
    }

    pub async fn revoke_api_token(
        &self,
        user_id: Uuid,
        token_id: Uuid,
    ) -> Result<RevokeTokenOutcome, ApiTokenStoreError> {
        let found: Option<Uuid> = sqlx::query_scalar(REVOKE_TOKEN_SQL)
            .bind(token_id)
            .bind(user_id)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        Ok(if found.is_some() {
            RevokeTokenOutcome::Revoked
        } else {
            RevokeTokenOutcome::NotFound
        })
    }

    pub async fn lookup_api_tokens(
        &self,
        prefix: &str,
        touch_after: Duration,
    ) -> Result<Vec<ApiTokenLookup>, ApiTokenStoreError> {
        let rows = sqlx::query(LOOKUP_TOKENS_SQL)
            .bind(prefix)
            .bind(touch_after.as_secs_f64())
            .bind(LOOKUP_LIMIT)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(lookup_from_row).collect()
    }

    pub async fn touch_api_token(
        &self,
        token_id: Uuid,
        min_interval: Duration,
    ) -> Result<bool, ApiTokenStoreError> {
        let outcome = sqlx::query(TOUCH_TOKEN_SQL)
            .bind(token_id)
            .bind(min_interval.as_secs_f64())
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected() > 0)
    }

    pub async fn delete_stale_api_tokens(
        &self,
        grace: Duration,
    ) -> Result<u64, ApiTokenStoreError> {
        let outcome = sqlx::query(DELETE_STALE_TOKENS_SQL)
            .bind(grace.as_secs_f64())
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected())
    }
}
