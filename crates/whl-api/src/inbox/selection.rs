use std::time::Duration;

use whl_auth::{AccessControl, AccessMode, AuthRejection, Caller, Decision, EndpointRole};
use whl_core::endpoint_id::EndpointId;
use whl_live::{INBOX_MAX_ANONYMOUS_ENDPOINTS, INBOX_MAX_ENDPOINTS};
use whl_store::{EndpointFilter, RoleFilter, Store, UserRecord};

use crate::error::{ApiError, FieldCode, FieldViolation};

const INBOX_ROLE: EndpointRole = EndpointRole::Viewer;

pub fn limit_for(caller: &Caller) -> usize {
    if caller.user().is_some() {
        INBOX_MAX_ENDPOINTS
    } else {
        INBOX_MAX_ANONYMOUS_ENDPOINTS
    }
}

fn violation(field: &str, code: FieldCode, message: &str) -> ApiError {
    ApiError::Validation(vec![FieldViolation::new(field, code, message)])
}

fn parse_list(text: &str, limit: usize) -> Result<Vec<EndpointId>, ApiError> {
    if text.is_empty() {
        return Err(violation(
            "endpoints",
            FieldCode::Required,
            "must name at least one endpoint",
        ));
    }
    let mut endpoints: Vec<EndpointId> = Vec::new();
    for token in text.split(',') {
        let endpoint = EndpointId::parse(token).map_err(|_| ApiError::NotFound)?;
        if !endpoints.contains(&endpoint) {
            if endpoints.len() >= limit {
                return Err(violation(
                    "endpoints",
                    FieldCode::TooMany,
                    &format!("must name at most {limit} endpoints"),
                ));
            }
            endpoints.push(endpoint);
        }
    }
    Ok(endpoints)
}

fn sign_in_rejection(access: &AccessControl) -> ApiError {
    match access.mode() {
        AccessMode::Mixed => AuthRejection::SignInRequired.into(),
        AccessMode::Open | AccessMode::Sso => AuthRejection::Unauthenticated.into(),
    }
}

fn default_user<'a>(
    access: &AccessControl,
    caller: &'a Caller,
) -> Result<&'a UserRecord, ApiError> {
    if !access.has_users() {
        return Err(violation(
            "endpoints",
            FieldCode::Required,
            "must name at least one endpoint",
        ));
    }
    caller.user().ok_or_else(|| sign_in_rejection(access))
}

async fn own_and_shared(
    store: &Store,
    retention: Duration,
    user: &UserRecord,
    limit: usize,
) -> Result<Vec<EndpointId>, ApiError> {
    let page = store
        .list_user_endpoints(
            user.id,
            retention,
            &EndpointFilter {
                role: RoleFilter::Any,
                tag: None,
                folder: None,
            },
            None,
            u32::try_from(limit).unwrap_or(u32::MAX),
            None,
        )
        .await?;
    Ok(page.items.into_iter().map(|item| item.record.id).collect())
}

async fn viewable(
    access: &AccessControl,
    store: &Store,
    retention: Duration,
    caller: &Caller,
    endpoints: &[EndpointId],
) -> Result<Vec<bool>, ApiError> {
    if endpoints.is_empty() {
        return Ok(Vec::new());
    }
    if access.mode() == AccessMode::Open {
        let live = store
            .live_endpoint_ids(endpoints, retention)
            .await
            .map_err(|error| ApiError::from(AuthRejection::from(error)))?;
        return Ok(endpoints
            .iter()
            .map(|endpoint| live.contains(endpoint.unpack()))
            .collect());
    }
    let roles = access.roles_on(caller, endpoints).await?;
    Ok(roles
        .into_iter()
        .map(|role| whl_auth::decide_access(role, INBOX_ROLE) == Decision::Allowed)
        .collect())
}

pub struct SelectionRequest<'a> {
    pub endpoints: Option<&'a str>,
    pub folder: Option<&'a str>,
    pub resumed: Option<Vec<EndpointId>>,
}

pub async fn select(
    access: &AccessControl,
    store: &Store,
    retention: Duration,
    caller: &Caller,
    request: SelectionRequest<'_>,
) -> Result<Vec<EndpointId>, ApiError> {
    if request.folder.is_some() {
        return Err(violation(
            "folder",
            FieldCode::NotAllowed,
            "folders are not available yet",
        ));
    }
    let limit = limit_for(caller);
    let strict = request.endpoints.is_some();
    let candidates = match request.endpoints {
        Some(text) => parse_list(text, limit)?,
        None => {
            let user = default_user(access, caller)?;
            match request.resumed {
                Some(resumed) if resumed.len() > limit => return Err(ApiError::InvalidCursor),
                Some(resumed) => resumed,
                None => own_and_shared(store, retention, user, limit).await?,
            }
        }
    };
    let allowed = viewable(access, store, retention, caller, &candidates).await?;
    if strict && allowed.iter().any(|allowed| !allowed) {
        return Err(ApiError::EndpointGone);
    }
    Ok(candidates
        .into_iter()
        .zip(allowed)
        .filter_map(|(endpoint, allowed)| allowed.then_some(endpoint))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "0123456789abcdefghjkmnpq";
    const SECOND: &str = "zzzzzzzzzzzzzzzzzzzzzzzz";

    #[test]
    fn a_list_keeps_first_occurrences_and_drops_duplicates() {
        let parsed = parse_list(&format!("{SECOND},{FIRST},{SECOND}"), 5).expect("parses");
        let texts: Vec<&str> = parsed.iter().map(EndpointId::unpack).collect();
        assert_eq!(texts, [SECOND, FIRST]);
    }

    #[test]
    fn a_list_over_the_bound_is_refused_but_duplicates_do_not_count() {
        assert!(parse_list(&format!("{FIRST},{SECOND}"), 2).is_ok());
        assert!(matches!(
            parse_list(&format!("{FIRST},{SECOND}"), 1),
            Err(ApiError::Validation(_))
        ));
        assert!(parse_list(&format!("{FIRST},{FIRST},{FIRST}"), 1).is_ok());
    }

    #[test]
    fn an_empty_or_malformed_list_is_refused() {
        assert!(matches!(parse_list("", 5), Err(ApiError::Validation(_))));
        assert!(matches!(
            parse_list(&format!("{FIRST},nope"), 5),
            Err(ApiError::NotFound)
        ));
        assert!(matches!(
            parse_list(&format!("{FIRST},"), 5),
            Err(ApiError::NotFound)
        ));
    }
}
