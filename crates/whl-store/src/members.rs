use std::time::Duration;

use sqlx::types::Uuid;
use sqlx::{Row, postgres::PgRow};
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;
use whl_core::tag::Tag;

use crate::auth::{EndpointRole, UserRecord, user_from_row};
use crate::captures::EVENT_CHANNEL;
use crate::endpoints::{EndpointError, EndpointRecord, record_from_row};
use crate::store::Store;

const QUALIFIED_COLUMNS: &str = "e.id, e.title, e.response_status, e.response_content_type, \
    e.response_headers::text AS response_headers, e.response_body, e.response_delay_ms, e.cors, \
    e.overflow, e.response_templated, e.response_status_template, \
    e.capture_count, e.bytes_used, e.last_seq, e.created_at, e.last_active_at, e.tags, \
    e.retention_seconds, e.never_expires, e.max_captures, e.capture_ttl_seconds, \
    e.verifier::text AS verifier";

const LIVE_ENDPOINT_SQL: &str = "SELECT owner_id FROM endpoints \
    WHERE id = $1 AND deleted_at IS NULL \
        AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
    FOR SHARE";

const FIND_USER_BY_EMAIL_SQL: &str = "SELECT id, email, display_name, tier FROM users \
    WHERE lower(email) = lower($1) AND email_verified \
    ORDER BY id \
    LIMIT 2";

const NOTIFY_SQL: &str = "SELECT pg_notify($1, $2)";

const UPSERT_MEMBER_SQL: &str = "INSERT INTO endpoint_members (endpoint_id, user_id, role) \
    VALUES ($1, $2, $3) \
    ON CONFLICT (endpoint_id, user_id) DO UPDATE SET role = EXCLUDED.role \
    RETURNING created_at";

const LIST_MEMBERS_SQL: &str = "SELECT u.id, u.email, u.display_name, u.tier, m.role, m.created_at \
    FROM endpoint_members m JOIN users u ON u.id = m.user_id \
    WHERE m.endpoint_id = $1 \
    ORDER BY m.created_at, u.id";

const DELETE_MEMBER_SQL: &str =
    "DELETE FROM endpoint_members WHERE endpoint_id = $1 AND user_id = $2";

const ENDPOINT_OWNER_SQL: &str = "SELECT u.id, u.email, u.display_name, u.tier \
    FROM endpoints e JOIN users u ON u.id = e.owner_id WHERE e.id = $1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantedRole {
    Viewer,
    Editor,
}

impl GrantedRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
        }
    }
}

impl From<GrantedRole> for EndpointRole {
    fn from(role: GrantedRole) -> Self {
        match role {
            GrantedRole::Viewer => Self::Viewer,
            GrantedRole::Editor => Self::Editor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFilter {
    pub role: RoleFilter,
    pub tag: Option<Tag>,
    pub folder: Option<Uuid>,
}

impl EndpointFilter {
    pub fn fingerprint(&self) -> String {
        format!(
            "{}:{}:{}",
            self.role.as_str(),
            self.tag.as_ref().map_or("-", Tag::unpack),
            self.folder
                .map_or_else(|| "-".to_owned(), |folder| folder.hyphenated().to_string()),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleFilter {
    Any,
    Owner,
    Shared,
}

impl RoleFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Owner => "owner",
            Self::Shared => "shared",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRecord {
    pub user: UserRecord,
    pub role: EndpointRole,
    pub added_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetMemberOutcome {
    Set(MemberRecord),
    EndpointGone,
    UserNotFound,
    EmailAmbiguous,
    TargetIsOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveMemberOutcome {
    Removed,
    NotMember,
    EndpointGone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListPosition {
    pub last_active_at: OffsetDateTime,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserEndpoint {
    pub record: EndpointRecord,
    pub role: EndpointRole,
    pub owner: Option<UserRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UserEndpointPage {
    pub items: Vec<UserEndpoint>,
    pub next: Option<ListPosition>,
}

#[derive(Debug, thiserror::Error)]
pub enum MembershipError {
    #[error("the database is unavailable: {0}")]
    Unavailable(#[source] sqlx::Error),
    #[error("membership query failed: {0}")]
    Query(#[source] sqlx::Error),
    #[error("stored membership data is invalid: {0}")]
    Invalid(String),
}

impl From<crate::auth::AuthStoreError> for MembershipError {
    fn from(error: crate::auth::AuthStoreError) -> Self {
        match error {
            crate::auth::AuthStoreError::Unavailable(source) => Self::Unavailable(source),
            crate::auth::AuthStoreError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

impl From<EndpointError> for MembershipError {
    fn from(error: EndpointError) -> Self {
        match error {
            EndpointError::Unavailable(source) => Self::Unavailable(source),
            EndpointError::Query(source) => Self::Query(source),
            other => Self::Invalid(other.to_string()),
        }
    }
}

fn classify(error: sqlx::Error) -> MembershipError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        MembershipError::Unavailable(error)
    } else {
        MembershipError::Query(error)
    }
}

fn member_role(text: &str) -> Result<EndpointRole, MembershipError> {
    match EndpointRole::parse(text) {
        Some(role) if role != EndpointRole::Owner => Ok(role),
        _ => Err(MembershipError::Invalid(format!(
            "unknown member role {text:?}"
        ))),
    }
}

fn member_from_row(row: &PgRow) -> Result<MemberRecord, MembershipError> {
    let role: String = row.try_get("role").map_err(classify)?;
    Ok(MemberRecord {
        user: user_from_row(row)?,
        role: member_role(&role)?,
        added_at: row.try_get("created_at").map_err(classify)?,
    })
}

impl Store {
    pub async fn list_members(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Vec<MemberRecord>, MembershipError> {
        let rows = sqlx::query(LIST_MEMBERS_SQL)
            .bind(endpoint.unpack())
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(member_from_row).collect()
    }

    pub async fn set_member(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        email: &str,
        role: GrantedRole,
    ) -> Result<SetMemberOutcome, MembershipError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let owner: Option<Option<Uuid>> = sqlx::query_scalar(LIVE_ENDPOINT_SQL)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        let Some(owner) = owner else {
            return Ok(SetMemberOutcome::EndpointGone);
        };
        let candidates = sqlx::query(FIND_USER_BY_EMAIL_SQL)
            .bind(email)
            .fetch_all(&mut *tx)
            .await
            .map_err(classify)?;
        let row = match candidates.as_slice() {
            [] => return Ok(SetMemberOutcome::UserNotFound),
            [only] => only,
            _ => return Ok(SetMemberOutcome::EmailAmbiguous),
        };
        let user = user_from_row(row)?;
        if owner == Some(user.id) {
            return Ok(SetMemberOutcome::TargetIsOwner);
        }
        let added_at: OffsetDateTime = sqlx::query_scalar(UPSERT_MEMBER_SQL)
            .bind(endpoint.unpack())
            .bind(user.id)
            .bind(role.as_str())
            .fetch_one(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(SetMemberOutcome::Set(MemberRecord {
            user,
            role: role.into(),
            added_at,
        }))
    }

    pub async fn remove_member(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
        user_id: Uuid,
    ) -> Result<RemoveMemberOutcome, MembershipError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let live: Option<Option<Uuid>> = sqlx::query_scalar(LIVE_ENDPOINT_SQL)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *tx)
            .await
            .map_err(classify)?;
        if live.is_none() {
            return Ok(RemoveMemberOutcome::EndpointGone);
        }
        let outcome = sqlx::query(DELETE_MEMBER_SQL)
            .bind(endpoint.unpack())
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        if outcome.rows_affected() == 0 {
            return Ok(RemoveMemberOutcome::NotMember);
        }
        let payload = serde_json::json!({ "t": "access_changed", "e": endpoint.unpack() });
        sqlx::query(NOTIFY_SQL)
            .bind(EVENT_CHANNEL)
            .bind(payload.to_string())
            .execute(&mut *tx)
            .await
            .map_err(classify)?;
        tx.commit().await.map_err(classify)?;
        Ok(RemoveMemberOutcome::Removed)
    }

    pub async fn endpoint_owner(
        &self,
        endpoint: &EndpointId,
    ) -> Result<Option<UserRecord>, MembershipError> {
        let row = sqlx::query(ENDPOINT_OWNER_SQL)
            .bind(endpoint.unpack())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)?;
        row.as_ref()
            .map(|row| user_from_row(row).map_err(MembershipError::from))
            .transpose()
    }

    pub async fn list_user_endpoints(
        &self,
        user_id: Uuid,
        retention: Duration,
        filter: &EndpointFilter,
        after: Option<&ListPosition>,
        limit: u32,
        only: Option<&[EndpointId]>,
    ) -> Result<UserEndpointPage, MembershipError> {
        let sql = format!(
            "SELECT {QUALIFIED_COLUMNS}, \
                CASE WHEN e.owner_id = $1 THEN 'owner' ELSE m.role END AS role, \
                ou.id AS owner_user_id, ou.email AS owner_email, ou.display_name AS owner_name, \
                ou.tier AS owner_tier \
             FROM endpoints e \
             LEFT JOIN endpoint_members m ON m.endpoint_id = e.id AND m.user_id = $1 \
             LEFT JOIN users ou ON ou.id = e.owner_id \
             WHERE e.deleted_at IS NULL \
                AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $2))) \
                AND (e.owner_id = $1 OR m.user_id IS NOT NULL) \
                AND ($3::text = 'any' \
                    OR ($3::text = 'owner' AND e.owner_id = $1) \
                    OR ($3::text = 'shared' AND e.owner_id IS DISTINCT FROM $1)) \
                AND ($4::timestamptz IS NULL OR (e.last_active_at, e.id) < ($4::timestamptz, $5::text)) \
                AND ($7::text[] IS NULL OR e.id = ANY($7::text[])) \
                AND ($8::text IS NULL OR $8::text = ANY(e.tags)) \
                AND ($9::uuid IS NULL OR EXISTS (SELECT 1 FROM folder_endpoints fe \
                    JOIN folders f ON f.id = fe.folder_id \
                    WHERE fe.folder_id = $9 AND fe.endpoint_id = e.id AND f.user_id = $1)) \
             ORDER BY e.last_active_at DESC, e.id DESC \
             LIMIT $6"
        );
        let fetch = i64::from(limit) + 1;
        let only: Option<Vec<String>> =
            only.map(|ids| ids.iter().map(|id| id.unpack().to_owned()).collect());
        let rows = sqlx::query(&sql)
            .bind(user_id)
            .bind(retention.as_secs_f64())
            .bind(filter.role.as_str())
            .bind(after.map(|position| position.last_active_at))
            .bind(after.map(|position| position.id.as_str()))
            .bind(fetch)
            .bind(only)
            .bind(filter.tag.as_ref().map(Tag::unpack))
            .bind(filter.folder)
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        let mut items = rows
            .iter()
            .map(|row| user_endpoint_from_row(row, retention))
            .collect::<Result<Vec<_>, _>>()?;
        let capacity = usize::try_from(limit).unwrap_or(usize::MAX);
        let next = if items.len() > capacity {
            items.truncate(capacity);
            items.last().map(|last| ListPosition {
                last_active_at: last.record.last_active_at,
                id: last.record.id.unpack().to_owned(),
            })
        } else {
            None
        };
        Ok(UserEndpointPage { items, next })
    }
}

fn user_endpoint_from_row(
    row: &PgRow,
    retention: Duration,
) -> Result<UserEndpoint, MembershipError> {
    let record = record_from_row(row, retention)?;
    let role_text: String = row.try_get("role").map_err(classify)?;
    let role = EndpointRole::parse(&role_text)
        .ok_or_else(|| MembershipError::Invalid(format!("unknown endpoint role {role_text:?}")))?;
    let owner_id: Option<Uuid> = row.try_get("owner_user_id").map_err(classify)?;
    let owner = match owner_id {
        Some(id) => Some(UserRecord {
            id,
            email: row.try_get("owner_email").map_err(classify)?,
            display_name: row.try_get("owner_name").map_err(classify)?,
            tier: row.try_get("owner_tier").map_err(classify)?,
        }),
        None => None,
    };
    Ok(UserEndpoint {
        record,
        role,
        owner,
    })
}
