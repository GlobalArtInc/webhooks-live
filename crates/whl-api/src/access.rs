use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, MacKey, Secret};
use whl_store::{AccessSettings, CaptureAuthRecord, StoredMac};

use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::input::{AccessPatch, CaptureAuthInput};
use crate::lifecycle::Standing;

pub fn resolve(
    keyring: Option<&Keyring>,
    standing: Standing,
    endpoint: &EndpointId,
    base: &AccessSettings,
    patch: &AccessPatch,
) -> Result<AccessSettings, ApiError> {
    if patch.is_empty() {
        return Ok(base.clone());
    }
    if !standing.may_change() {
        return Err(ApiError::OwnerRequired);
    }
    if !standing.owned {
        return Err(ApiError::Validation(ownerless_violations(patch)));
    }
    let sender_allow = patch
        .sender_allow
        .clone()
        .unwrap_or_else(|| base.sender_allow.clone());
    let capture_auth = match &patch.capture_auth {
        None => base.capture_auth.clone(),
        Some(None) => None,
        Some(Some(input)) => Some(seal(keyring, endpoint, input)?),
    };
    Ok(AccessSettings {
        sender_allow,
        capture_auth,
    })
}

fn ownerless_violations(patch: &AccessPatch) -> Vec<FieldViolation> {
    let message = "only an endpoint with an owner can restrict who may send";
    let mut violations = Vec::new();
    if patch.sender_allow.is_some() {
        violations.push(FieldViolation::new(
            "sender_allow_cidrs",
            FieldCode::NotAllowed,
            message,
        ));
    }
    if patch.capture_auth.is_some() {
        violations.push(FieldViolation::new(
            "capture_auth",
            FieldCode::NotAllowed,
            message,
        ));
    }
    violations
}

fn seal(
    keyring: Option<&Keyring>,
    endpoint: &EndpointId,
    input: &CaptureAuthInput,
) -> Result<CaptureAuthRecord, ApiError> {
    let keyring = keyring.ok_or(ApiError::SecretsDisabled)?;
    let (key_id, key) = keyring
        .active_capture_mac_key(endpoint)
        .map_err(ApiError::CaptureAuthMac)?;
    Ok(match input {
        CaptureAuthInput::Basic { username, password } => CaptureAuthRecord::Basic {
            key_id,
            username: username.clone(),
            password_mac: mac(&key, password)?,
        },
        CaptureAuthInput::Header { name, value } => CaptureAuthRecord::Header {
            key_id,
            name: name.clone(),
            value_mac: mac(&key, value)?,
        },
    })
}

fn mac(key: &MacKey, secret: &Secret) -> Result<StoredMac, ApiError> {
    let tag = key
        .tag(secret.unpack().as_bytes())
        .map_err(ApiError::CaptureAuthMac)?;
    Ok(StoredMac::new(*tag.unpack()))
}
