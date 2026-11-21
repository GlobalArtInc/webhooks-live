use std::fmt;
use std::time::Duration;

use sqlx::types::Uuid;
use sqlx::{Row, postgres::PgRow};
use whl_core::creator_net::CreatorNet;
use whl_core::endpoint_id::EndpointId;

use crate::store::Store;

const UPSERT_USER_SQL: &str = "INSERT INTO users (id, issuer, subject, email, email_verified, display_name) \
    VALUES (gen_random_uuid(), $1, $2, $3, $4, $5) \
    ON CONFLICT (issuer, subject) DO UPDATE \
    SET email = EXCLUDED.email, email_verified = EXCLUDED.email_verified, \
        display_name = EXCLUDED.display_name, last_login_at = now() \
    RETURNING id, email, display_name";

const CREATE_SESSION_SQL: &str = "INSERT INTO sessions (token_hash, user_id, expires_at) \
    VALUES ($1, $2, now() + make_interval(secs => $3))";

const SESSION_USER_SQL: &str = "WITH live AS ( \
        SELECT token_hash, user_id FROM sessions \
        WHERE token_hash = $1 AND expires_at > now() \
            AND last_seen_at > now() - make_interval(secs => $2) \
    ), touched AS ( \
        UPDATE sessions SET last_seen_at = now() \
        WHERE token_hash IN (SELECT token_hash FROM live) \
            AND last_seen_at < now() - make_interval(secs => $3) \
        RETURNING token_hash \
    ) \
    SELECT u.id, u.email, u.display_name FROM live JOIN users u ON u.id = live.user_id";

const DELETE_SESSION_SQL: &str = "DELETE FROM sessions WHERE token_hash = $1";

const DELETE_EXPIRED_SESSIONS_SQL: &str = "DELETE FROM sessions \
    WHERE expires_at <= now() OR last_seen_at <= now() - make_interval(secs => $1)";

const CREATE_LOGIN_STATE_SQL: &str = "WITH counts AS ( \
        SELECT count(*) AS total, \
            count(*) FILTER (WHERE client_net = $7::text::cidr) AS network \
        FROM login_states WHERE expires_at > now() \
    ), inserted AS ( \
        INSERT INTO login_states (state, pkce_verifier, nonce, return_to, expires_at, client_net) \
        SELECT $1::text, $2::text, $3::text, $4::text, now() + make_interval(secs => $5), \
            $7::text::cidr \
        FROM counts WHERE total < $6::bigint AND network < $8::bigint \
        RETURNING 1 \
    ) \
    SELECT total, network, (SELECT count(*) FROM inserted) AS inserted FROM counts";

const TAKE_LOGIN_STATE_SQL: &str = "DELETE FROM login_states \
    WHERE state = $1 AND expires_at > now() RETURNING pkce_verifier, nonce, return_to";

const DELETE_EXPIRED_LOGIN_STATES_SQL: &str = "DELETE FROM login_states WHERE expires_at <= now()";

const ENDPOINT_ROLE_SQL: &str = "SELECT CASE WHEN e.owner_id = $2 THEN 'owner' ELSE m.role END AS role \
    FROM endpoints e \
    LEFT JOIN endpoint_members m ON m.endpoint_id = e.id AND m.user_id = $2 \
    WHERE e.id = $1 AND e.deleted_at IS NULL \
        AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $3))) \
        AND (e.owner_id = $2 OR m.user_id IS NOT NULL)";

const ENDPOINT_STANDING_SQL: &str = "SELECT e.owner_id IS NOT NULL AS owned, \
        CASE WHEN e.owner_id IS NULL OR e.owner_id = $2 THEN 'owner' ELSE m.role END AS role \
    FROM endpoints e \
    LEFT JOIN endpoint_members m ON m.endpoint_id = e.id AND m.user_id = $2 \
    WHERE e.id = $1 AND e.deleted_at IS NULL \
        AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $3)))";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointStanding {
    pub owned: bool,
    pub role: Option<EndpointRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EndpointRole {
    Viewer,
    Editor,
    Owner,
}

impl EndpointRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub display_name: Option<String>,
}

impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedIdentity")
            .field("issuer", &self.issuer)
            .field("subject", &"<redacted>")
            .field("email", &"<redacted>")
            .field("email_verified", &self.email_verified)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
}

impl fmt::Debug for UserRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserRecord")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LoginRecord {
    pub pkce_verifier: String,
    pub nonce: String,
    pub return_to: String,
}

impl fmt::Debug for LoginRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRecord")
            .field("return_to", &self.return_to)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLifetime {
    pub idle: Duration,
    pub touch_after: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthStoreError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("auth query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored auth data is invalid: {0}")]
    Invalid(String),
    #[error("too many sign-ins are pending")]
    LoginsFull,
    #[error("too many sign-ins are pending from this network")]
    LoginsFullForNetwork,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginCaps {
    pub total: u32,
    pub per_network: u32,
}

pub(crate) fn classify(error: sqlx::Error) -> AuthStoreError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        AuthStoreError::Unavailable(error)
    } else {
        AuthStoreError::Query(error)
    }
}

pub(crate) fn user_from_row(row: &PgRow) -> Result<UserRecord, AuthStoreError> {
    Ok(UserRecord {
        id: row.try_get("id").map_err(classify)?,
        email: row.try_get("email").map_err(classify)?,
        display_name: row.try_get("display_name").map_err(classify)?,
    })
}

impl Store {
    pub async fn upsert_user(
        &self,
        identity: &VerifiedIdentity,
    ) -> Result<UserRecord, AuthStoreError> {
        let row = sqlx::query(UPSERT_USER_SQL)
            .bind(&identity.issuer)
            .bind(&identity.subject)
            .bind(&identity.email)
            .bind(identity.email_verified)
            .bind(identity.display_name.as_deref())
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        user_from_row(&row)
    }

    pub async fn create_session(
        &self,
        token_hash: &[u8],
        user_id: Uuid,
        max: Duration,
    ) -> Result<(), AuthStoreError> {
        sqlx::query(CREATE_SESSION_SQL)
            .bind(token_hash)
            .bind(user_id)
            .bind(max.as_secs_f64())
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(())
    }

    pub async fn session_user(
        &self,
        token_hash: &[u8],
        lifetime: SessionLifetime,
    ) -> Result<Option<UserRecord>, AuthStoreError> {
        let row = sqlx::query(SESSION_USER_SQL)
            .bind(token_hash)
            .bind(lifetime.idle.as_secs_f64())
            .bind(lifetime.touch_after.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref().map(user_from_row).transpose()
    }

    pub async fn delete_session(&self, token_hash: &[u8]) -> Result<bool, AuthStoreError> {
        let outcome = sqlx::query(DELETE_SESSION_SQL)
            .bind(token_hash)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected() > 0)
    }

    pub async fn delete_expired_sessions(&self, idle: Duration) -> Result<u64, AuthStoreError> {
        let outcome = sqlx::query(DELETE_EXPIRED_SESSIONS_SQL)
            .bind(idle.as_secs_f64())
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected())
    }

    pub async fn create_login_state(
        &self,
        key: &str,
        login: &LoginRecord,
        ttl: Duration,
        caps: LoginCaps,
        client_net: Option<&CreatorNet>,
    ) -> Result<(), AuthStoreError> {
        let row = sqlx::query(CREATE_LOGIN_STATE_SQL)
            .bind(key)
            .bind(&login.pkce_verifier)
            .bind(&login.nonce)
            .bind(&login.return_to)
            .bind(ttl.as_secs_f64())
            .bind(i64::from(caps.total))
            .bind(client_net.map(ToString::to_string))
            .bind(i64::from(caps.per_network))
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        let inserted: i64 = row.try_get("inserted").map_err(classify)?;
        if inserted > 0 {
            return Ok(());
        }
        let total: i64 = row.try_get("total").map_err(classify)?;
        if total >= i64::from(caps.total) {
            Err(AuthStoreError::LoginsFull)
        } else {
            Err(AuthStoreError::LoginsFullForNetwork)
        }
    }

    pub async fn take_login_state(&self, key: &str) -> Result<Option<LoginRecord>, AuthStoreError> {
        let row = sqlx::query(TAKE_LOGIN_STATE_SQL)
            .bind(key)
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.map(|row| {
            Ok(LoginRecord {
                pkce_verifier: row.try_get("pkce_verifier").map_err(classify)?,
                nonce: row.try_get("nonce").map_err(classify)?,
                return_to: row.try_get("return_to").map_err(classify)?,
            })
        })
        .transpose()
    }

    pub async fn delete_expired_login_states(&self) -> Result<u64, AuthStoreError> {
        let outcome = sqlx::query(DELETE_EXPIRED_LOGIN_STATES_SQL)
            .execute(self.pool())
            .await
            .map_err(classify)?;
        Ok(outcome.rows_affected())
    }

    pub async fn endpoint_role(
        &self,
        endpoint: &EndpointId,
        user_id: Uuid,
        retention: Duration,
    ) -> Result<Option<EndpointRole>, AuthStoreError> {
        let row = sqlx::query(ENDPOINT_ROLE_SQL)
            .bind(endpoint.unpack())
            .bind(user_id)
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let text: String = row.try_get("role").map_err(classify)?;
        EndpointRole::parse(&text)
            .map(Some)
            .ok_or_else(|| AuthStoreError::Invalid(format!("unknown endpoint role {text:?}")))
    }

    pub async fn endpoint_standing(
        &self,
        endpoint: &EndpointId,
        user_id: Option<Uuid>,
        retention: Duration,
    ) -> Result<Option<EndpointStanding>, AuthStoreError> {
        let row = sqlx::query(ENDPOINT_STANDING_SQL)
            .bind(endpoint.unpack())
            .bind(user_id)
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let owned: bool = row.try_get("owned").map_err(classify)?;
        let text: Option<String> = row.try_get("role").map_err(classify)?;
        let role = match text {
            Some(text) => Some(EndpointRole::parse(&text).ok_or_else(|| {
                AuthStoreError::Invalid(format!("unknown endpoint role {text:?}"))
            })?),
            None => None,
        };
        Ok(Some(EndpointStanding { owned, role }))
    }
}
