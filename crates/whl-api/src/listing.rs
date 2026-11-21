use axum::Json;
use axum::extract::{Query, State};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_core::tag::Tag;
use whl_store::{EndpointFilter, ListPosition, RoleFilter, Uuid};

use crate::dto::EndpointDto;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

#[derive(Debug, Default, Deserialize)]
pub struct ListParams {
    limit: Option<String>,
    cursor: Option<String>,
    role: Option<String>,
    tag: Option<String>,
    folder: Option<String>,
}

#[derive(Serialize)]
pub struct EndpointListDto {
    items: Vec<EndpointDto>,
    next_cursor: Option<String>,
}

pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
    Query(params): Query<ListParams>,
) -> Result<Json<EndpointListDto>, ApiError> {
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    let user = caller.user().ok_or(AuthRejection::Unauthenticated)?;
    let query = ListQuery::parse(&params)?;
    let page = state
        .store
        .list_user_endpoints(
            user.id,
            state.settings.retention,
            &query.filter,
            query.after.as_ref(),
            query.limit,
            caller.token().and_then(|grant| grant.endpoint_limit()),
        )
        .await?;
    let ids: Vec<EndpointId> = page
        .items
        .iter()
        .map(|item| item.record.id.clone())
        .collect();
    let mut rules = state.store.list_forward_rules_for(&ids).await?;
    let mut items = Vec::with_capacity(page.items.len());
    for item in &page.items {
        let own = rules.remove(&item.record.id).unwrap_or_default();
        items.push(
            EndpointDto::from_record(&item.record, &state.settings, &own)
                .with_identity(Some(item.role), item.owner.as_ref()),
        );
    }
    let next_cursor = page
        .next
        .as_ref()
        .map(|position| encode_cursor(&query.filter, position))
        .transpose()?;
    Ok(Json(EndpointListDto { items, next_cursor }))
}

struct ListQuery {
    filter: EndpointFilter,
    limit: u32,
    after: Option<ListPosition>,
}

impl ListQuery {
    fn parse(params: &ListParams) -> Result<Self, ApiError> {
        let mut violations = Vec::new();
        let role = match params.role.as_deref() {
            None => RoleFilter::Any,
            Some("owner") => RoleFilter::Owner,
            Some("shared") => RoleFilter::Shared,
            Some(_) => {
                violations.push(FieldViolation::new(
                    "role",
                    FieldCode::InvalidFormat,
                    "must be owner or shared",
                ));
                RoleFilter::Any
            }
        };
        let tag = match params.tag.as_deref().map(Tag::parse) {
            None => None,
            Some(Ok(tag)) => Some(tag),
            Some(Err(_)) => {
                violations.push(FieldViolation::new(
                    "tag",
                    FieldCode::InvalidFormat,
                    "must be a tag of 1 to 32 characters of a to z, 0 to 9 and hyphen",
                ));
                None
            }
        };
        let folder = match params.folder.as_deref().map(Uuid::try_parse) {
            None => None,
            Some(Ok(folder)) => Some(folder),
            Some(Err(_)) => {
                violations.push(FieldViolation::new(
                    "folder",
                    FieldCode::InvalidFormat,
                    "must be a folder id",
                ));
                None
            }
        };
        let filter = EndpointFilter { role, tag, folder };
        let limit = match params.limit.as_deref() {
            None => DEFAULT_LIMIT,
            Some(text) => match text.parse::<u32>() {
                Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => limit,
                Ok(_) => {
                    violations.push(FieldViolation::new(
                        "limit",
                        FieldCode::OutOfRange,
                        "must be between 1 and 200",
                    ));
                    DEFAULT_LIMIT
                }
                Err(_) => {
                    violations.push(FieldViolation::new(
                        "limit",
                        FieldCode::InvalidFormat,
                        "must be an integer",
                    ));
                    DEFAULT_LIMIT
                }
            },
        };
        if !violations.is_empty() {
            return Err(ApiError::Validation(violations));
        }
        let after = match params.cursor.as_deref() {
            Some(text) => Some(decode_cursor(&filter, text).ok_or(ApiError::InvalidCursor)?),
            None => None,
        };
        Ok(Self {
            filter,
            limit,
            after,
        })
    }
}

fn encode_cursor(filter: &EndpointFilter, position: &ListPosition) -> Result<String, ApiError> {
    let micros = i64::try_from(position.last_active_at.unix_timestamp_nanos() / 1000)
        .map_err(|_| ApiError::InvalidCursor)?;
    let text = format!("{}|{micros}|{}", filter.fingerprint(), position.id);
    Ok(URL_SAFE_NO_PAD.encode(text))
}

fn decode_cursor(filter: &EndpointFilter, text: &str) -> Option<ListPosition> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    let decoded = String::from_utf8(bytes).ok()?;
    let mut parts = decoded.splitn(3, '|');
    if parts.next()? != filter.fingerprint() {
        return None;
    }
    let micros: i64 = parts.next()?.parse().ok()?;
    let id = EndpointId::parse(parts.next()?).ok()?;
    let last_active_at =
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(micros).checked_mul(1000)?).ok()?;
    Some(ListPosition {
        last_active_at,
        id: id.unpack().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdefghjkmnpq";

    fn filter(role: RoleFilter) -> EndpointFilter {
        EndpointFilter {
            role,
            tag: None,
            folder: None,
        }
    }

    fn position() -> ListPosition {
        ListPosition {
            last_active_at: OffsetDateTime::from_unix_timestamp_nanos(1_700_000_000_123_456_000)
                .expect("a valid timestamp"),
            id: ID.to_owned(),
        }
    }

    #[test]
    fn a_cursor_round_trips_with_microsecond_precision() {
        let cursor = encode_cursor(&filter(RoleFilter::Shared), &position()).expect("encodes");
        assert_eq!(
            decode_cursor(&filter(RoleFilter::Shared), &cursor),
            Some(position())
        );
    }

    #[test]
    fn a_cursor_is_refused_under_another_filter_or_when_malformed() {
        let cursor = encode_cursor(&filter(RoleFilter::Shared), &position()).expect("encodes");
        assert_eq!(decode_cursor(&filter(RoleFilter::Owner), &cursor), None);
        assert_eq!(decode_cursor(&filter(RoleFilter::Any), &cursor), None);
        for text in [
            "",
            "!!!",
            &URL_SAFE_NO_PAD.encode("any|1"),
            &URL_SAFE_NO_PAD.encode("any|x|0123456789abcdefghjkmnpq"),
            &URL_SAFE_NO_PAD.encode("any|1|not-an-endpoint-id"),
            &URL_SAFE_NO_PAD.encode("any|99999999999999999999999|0123456789abcdefghjkmnpq"),
        ] {
            assert_eq!(
                decode_cursor(&filter(RoleFilter::Any), text),
                None,
                "{text:?}"
            );
        }
    }
}
