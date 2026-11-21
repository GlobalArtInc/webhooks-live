use whl_auth::AuthRejection;
use whl_core::tier::Tier;
use whl_store::lifecycle::{MIN_CAPTURE_TTL_SECONDS, MIN_RETENTION_SECONDS};
use whl_store::{AccessSettings, EndpointChange, EndpointRole, EndpointSettings, Lifecycle};

use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::input::{LifecyclePatch, RetentionPatch};
use crate::settings::ApiSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub owned: bool,
    pub signed_in: bool,
    pub sign_in_possible: bool,
    pub role: Option<EndpointRole>,
}

impl Standing {
    pub(crate) fn may_change(self) -> bool {
        matches!(self.role, None | Some(EndpointRole::Owner))
    }
}

pub fn resolve(
    settings: &ApiSettings,
    tier: Tier<'_>,
    standing: Standing,
    base: &Lifecycle,
    patch: &LifecyclePatch,
) -> Result<Lifecycle, ApiError> {
    if patch.is_empty() {
        return Ok(*base);
    }
    if !standing.may_change() {
        return Err(ApiError::OwnerRequired);
    }
    let limits = settings.lifecycle_limits(tier);
    let mut violations = Vec::new();
    let mut lifecycle = *base;
    match patch.retention {
        None => {}
        Some(RetentionPatch::Never) => {
            if !standing.owned {
                if !standing.signed_in && standing.sign_in_possible {
                    return Err(ApiError::Auth(AuthRejection::SignInRequired));
                }
                violations.push(FieldViolation::new(
                    "retention.mode",
                    FieldCode::NotAllowed,
                    "an endpoint without an owner cannot be set to never expire",
                ));
            }
            lifecycle.never_expires = true;
            lifecycle.retention_seconds = None;
        }
        Some(RetentionPatch::Sliding { seconds }) => {
            lifecycle.never_expires = false;
            lifecycle.retention_seconds = match seconds {
                None => None,
                Some(value) => bounded(
                    &mut violations,
                    "retention.seconds",
                    value,
                    i64::from(MIN_RETENTION_SECONDS),
                    limits.max_retention_seconds,
                ),
            };
        }
    }
    if let Some(requested) = patch.max_captures {
        lifecycle.max_captures = match requested {
            None => None,
            Some(value) => bounded(
                &mut violations,
                "max_captures",
                value,
                1,
                i64::from(limits.max_captures),
            ),
        };
    }
    if let Some(requested) = patch.capture_ttl_seconds {
        lifecycle.capture_ttl_seconds = match requested {
            None => None,
            Some(value) => bounded(
                &mut violations,
                "capture_ttl_seconds",
                value,
                i64::from(MIN_CAPTURE_TTL_SECONDS),
                i64::from(i32::MAX),
            ),
        };
    }
    if violations.is_empty() {
        Ok(lifecycle)
    } else {
        Err(ApiError::Validation(violations))
    }
}

pub fn combine(
    settings: Result<EndpointSettings, Vec<FieldViolation>>,
    lifecycle: Result<Lifecycle, ApiError>,
    access: Result<AccessSettings, ApiError>,
) -> Result<EndpointChange, ApiError> {
    let lifecycle = match lifecycle {
        Err(error) if is_decisive(&error) => return Err(error),
        other => other,
    };
    let access = match access {
        Err(error) if is_decisive(&error) => return Err(error),
        other => other,
    };
    match (settings, lifecycle, access) {
        (Ok(settings), Ok(lifecycle), Ok(access)) => Ok(EndpointChange {
            settings,
            lifecycle,
            access,
        }),
        (settings, lifecycle, access) => {
            let mut violations = settings.err().unwrap_or_default();
            let mut other = None;
            for error in [lifecycle.err(), access.err()].into_iter().flatten() {
                match error {
                    ApiError::Validation(more) => violations.extend(more),
                    error => other = other.or(Some(error)),
                }
            }
            match other {
                Some(error) if violations.is_empty() => Err(error),
                _ => Err(ApiError::Validation(violations)),
            }
        }
    }
}

fn is_decisive(error: &ApiError) -> bool {
    matches!(error, ApiError::Auth(_) | ApiError::OwnerRequired)
}

fn bounded(
    violations: &mut Vec<FieldViolation>,
    field: &str,
    value: i64,
    min: i64,
    max: i64,
) -> Option<i32> {
    match i32::try_from(value) {
        Ok(narrow) if value >= min && value <= max => Some(narrow),
        _ => {
            violations.push(FieldViolation::new(
                field,
                FieldCode::OutOfRange,
                format!("must be between {min} and {max}"),
            ));
            None
        }
    }
}
