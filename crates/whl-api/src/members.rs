use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::{Map, Value};
use whl_core::endpoint_id::EndpointId;
use whl_store::{GrantedRole, MemberRecord, RemoveMemberOutcome, SetMemberOutcome, Uuid};

use crate::body::RawBody;
use crate::dto::{UserDto, timestamp};
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::routing::{Routes, delete, get};
use crate::state::ApiState;

const MAX_EMAIL_BYTES: usize = 254;
const MEMBER_KEYS: &[&str] = &["email", "role"];

pub fn routes() -> Routes {
    Routes::new()
        .route("/endpoints/{id}/members", get(list).put(set))
        .route("/endpoints/{id}/members/{user_id}", delete(remove))
}

#[derive(Serialize)]
struct MemberDto {
    user: UserDto,
    role: &'static str,
    added_at: String,
}

impl From<&MemberRecord> for MemberDto {
    fn from(member: &MemberRecord) -> Self {
        Self {
            user: UserDto::from(&member.user),
            role: member.role.as_str(),
            added_at: timestamp(member.added_at),
        }
    }
}

#[derive(Serialize)]
struct MemberListDto {
    items: Vec<MemberDto>,
}

#[derive(Debug, PartialEq, Eq)]
struct MemberInput {
    email: String,
    role: GrantedRole,
}

async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<MemberListDto>, ApiError> {
    ensure_users(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let members = state.store.list_members(&endpoint).await?;
    Ok(Json(MemberListDto {
        items: members.iter().map(MemberDto::from).collect(),
    }))
}

async fn set(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<MemberDto>, ApiError> {
    ensure_users(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let input = parse_member_input(&body)?;
    let outcome = state
        .store
        .set_member(
            &endpoint,
            state.settings.retention,
            &input.email,
            input.role,
        )
        .await?;
    match outcome {
        SetMemberOutcome::Set(member) => Ok(Json(MemberDto::from(&member))),
        SetMemberOutcome::EndpointGone => Err(ApiError::EndpointGone),
        SetMemberOutcome::UserNotFound => Err(ApiError::UserNotFound),
        SetMemberOutcome::EmailAmbiguous => Err(ApiError::EmailAmbiguous),
        SetMemberOutcome::TargetIsOwner => Err(ApiError::Validation(vec![FieldViolation::new(
            "email",
            FieldCode::NotAllowed,
            "the owner cannot be added as a member",
        )])),
    }
}

async fn remove(
    State(state): State<ApiState>,
    Path((raw_id, raw_user)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ensure_users(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let user = Uuid::try_parse(&raw_user)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw_user)
        .ok_or(ApiError::NotFound)?;
    match state
        .store
        .remove_member(&endpoint, state.settings.retention, user)
        .await?
    {
        RemoveMemberOutcome::Removed => Ok(StatusCode::NO_CONTENT),
        RemoveMemberOutcome::NotMember => Err(ApiError::MemberNotFound),
        RemoveMemberOutcome::EndpointGone => Err(ApiError::EndpointGone),
    }
}

fn ensure_users(state: &ApiState) -> Result<(), ApiError> {
    if state.access.has_users() {
        Ok(())
    } else {
        Err(ApiError::NotAvailableInMode)
    }
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

async fn live_endpoint(state: &ApiState, endpoint: &EndpointId) -> Result<(), ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .map(|_| ())
        .ok_or(ApiError::EndpointGone)
}

fn parse_member_input(bytes: &[u8]) -> Result<MemberInput, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidJson("the request body is empty"));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?;
    let Value::Object(object) = value else {
        return Err(ApiError::InvalidJson(
            "the request body must be a JSON object",
        ));
    };
    let mut violations = Vec::new();
    for key in object
        .keys()
        .filter(|key| !MEMBER_KEYS.contains(&key.as_str()))
    {
        violations.push(FieldViolation::new(
            key.clone(),
            FieldCode::NotAllowed,
            "is not a known field",
        ));
    }
    let email = member_email(&object, &mut violations);
    let role = member_role(&object, &mut violations);
    match (email, role) {
        (Some(email), Some(role)) if violations.is_empty() => Ok(MemberInput { email, role }),
        _ => Err(ApiError::Validation(violations)),
    }
}

fn member_email(
    object: &Map<String, Value>,
    violations: &mut Vec<FieldViolation>,
) -> Option<String> {
    let Some(value) = object.get("email") else {
        violations.push(FieldViolation::new(
            "email",
            FieldCode::Required,
            "is required",
        ));
        return None;
    };
    let Some(text) = value.as_str() else {
        violations.push(FieldViolation::new(
            "email",
            FieldCode::InvalidFormat,
            "must be a string",
        ));
        return None;
    };
    let email = text.trim();
    if email.is_empty() {
        violations.push(FieldViolation::new(
            "email",
            FieldCode::Required,
            "is required",
        ));
        return None;
    }
    if email.len() > MAX_EMAIL_BYTES {
        violations.push(FieldViolation::new(
            "email",
            FieldCode::TooLong,
            format!("must be at most {MAX_EMAIL_BYTES} bytes"),
        ));
        return None;
    }
    let well_formed = email
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
        && !email
            .chars()
            .any(|character| character.is_control() || character.is_whitespace());
    if !well_formed {
        violations.push(FieldViolation::new(
            "email",
            FieldCode::InvalidFormat,
            "must be an email address",
        ));
        return None;
    }
    Some(email.to_owned())
}

fn member_role(
    object: &Map<String, Value>,
    violations: &mut Vec<FieldViolation>,
) -> Option<GrantedRole> {
    let Some(value) = object.get("role") else {
        violations.push(FieldViolation::new(
            "role",
            FieldCode::Required,
            "is required",
        ));
        return None;
    };
    match value.as_str() {
        Some("viewer") => Some(GrantedRole::Viewer),
        Some("editor") => Some(GrantedRole::Editor),
        _ => {
            violations.push(FieldViolation::new(
                "role",
                FieldCode::InvalidFormat,
                "must be viewer or editor",
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<MemberInput, Vec<(String, FieldCode)>> {
        match parse_member_input(text.as_bytes()) {
            Ok(input) => Ok(input),
            Err(ApiError::Validation(violations)) => Err(violations
                .iter()
                .map(|violation| (violation.field().to_owned(), violation.code()))
                .collect()),
            Err(other) => panic!("unexpected error {other}"),
        }
    }

    #[test]
    fn a_well_formed_body_is_accepted_and_the_email_is_trimmed() {
        let input = parse(r#"{"email": "  Dev@Example.test ", "role": "editor"}"#).expect("valid");
        assert_eq!(input.email, "Dev@Example.test");
        assert_eq!(input.role, GrantedRole::Editor);
    }

    #[test]
    fn every_bad_field_is_reported_together() {
        let all = parse(r#"{"email": 3, "role": "owner", "extra": 1}"#).expect_err("invalid");
        assert!(all.contains(&("email".to_owned(), FieldCode::InvalidFormat)));
        assert!(all.contains(&("role".to_owned(), FieldCode::InvalidFormat)));
        assert!(all.contains(&("extra".to_owned(), FieldCode::NotAllowed)));
        let missing = parse("{}").expect_err("invalid");
        assert!(missing.contains(&("email".to_owned(), FieldCode::Required)));
        assert!(missing.contains(&("role".to_owned(), FieldCode::Required)));
    }

    #[test]
    fn malformed_and_oversized_emails_are_refused() {
        for email in [
            "plain",
            "@x.test",
            "a@",
            "a b@x.test",
            "a\\u0000b@x.test",
            "",
        ] {
            let body = format!(r#"{{"email": "{email}", "role": "viewer"}}"#);
            assert!(parse(&body).is_err(), "{email:?}");
        }
        let long = format!("{}@x.test", "a".repeat(MAX_EMAIL_BYTES));
        let body = format!(r#"{{"email": "{long}", "role": "viewer"}}"#);
        let errors = parse(&body).expect_err("too long");
        assert_eq!(errors, [("email".to_owned(), FieldCode::TooLong)]);
    }

    #[test]
    fn a_body_that_is_not_an_object_is_invalid_json() {
        for body in ["", "[]", "\"x\"", "not json"] {
            assert!(matches!(
                parse_member_input(body.as_bytes()),
                Err(ApiError::InvalidJson(_))
            ));
        }
    }
}
