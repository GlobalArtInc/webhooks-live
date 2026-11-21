use std::net::IpAddr;

use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use whl_core::capture_auth::MAX_CREDENTIAL_BYTES;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, MacKey, constant_time_eq};
use whl_store::{AccessSettings, CaptureAuthRecord};
use whl_telemetry::series;

use crate::refusal::Refusal;
use crate::response::{ReplyRequest, preflight_reply};

const MAX_BASIC_HEADER_BYTES: usize = 1024;
const BASIC_SCHEME: &[u8] = b"basic";

pub(crate) enum Admit {
    Proceed,
    Preflight(Response),
}

enum Verdict {
    Match,
    Mismatch,
    Unverifiable,
}

pub(crate) fn admit(
    access: &AccessSettings,
    cors: bool,
    endpoint: &EndpointId,
    client_ip: IpAddr,
    request: &ReplyRequest<'_>,
    keyring: Option<&Keyring>,
) -> Result<Admit, Refusal> {
    if !access.sender_allow.permits(client_ip) {
        count("sender_refused");
        return Err(Refusal::SenderRefused);
    }
    let Some(record) = &access.capture_auth else {
        return Ok(Admit::Proceed);
    };
    if cors && let Some(response) = preflight_reply(endpoint, request) {
        return Ok(Admit::Preflight(response));
    }
    match check_credentials(record, endpoint, request.headers, keyring) {
        Verdict::Match => Ok(Admit::Proceed),
        Verdict::Mismatch => {
            count("unauthorized");
            Err(Refusal::Unauthorized)
        }
        Verdict::Unverifiable => {
            count("unverifiable");
            Err(Refusal::Unauthorized)
        }
    }
}

fn count(result: &'static str) {
    metrics::counter!(series::CAPTURES_TOTAL, "result" => result).increment(1);
}

fn check_credentials(
    record: &CaptureAuthRecord,
    endpoint: &EndpointId,
    headers: &HeaderMap,
    keyring: Option<&Keyring>,
) -> Verdict {
    let Some(key) =
        keyring.and_then(|keyring| keyring.capture_mac_key(record.key_id(), endpoint).ok())
    else {
        return Verdict::Unverifiable;
    };
    let matched = match record {
        CaptureAuthRecord::Basic {
            username,
            password_mac,
            ..
        } => basic_matches(&key, username.unpack(), password_mac.unpack(), headers),
        CaptureAuthRecord::Header {
            name, value_mac, ..
        } => header_matches(&key, name.unpack(), value_mac.unpack(), headers),
    };
    if matched {
        Verdict::Match
    } else {
        Verdict::Mismatch
    }
}

fn basic_matches(key: &MacKey, username: &str, password_mac: &[u8], headers: &HeaderMap) -> bool {
    let Some((presented_user, presented_password)) = basic_credentials(headers) else {
        return false;
    };
    let user_matches = constant_time_eq(&presented_user, username.as_bytes());
    let password_matches = key.verify(&presented_password, password_mac);
    user_matches & password_matches
}

fn header_matches(key: &MacKey, name: &str, value_mac: &[u8], headers: &HeaderMap) -> bool {
    let Some(value) = single_value(headers, name) else {
        return false;
    };
    let presented = value.as_bytes();
    if presented.len() > MAX_CREDENTIAL_BYTES {
        return false;
    }
    key.verify(presented, value_mac)
}

fn single_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a HeaderValue> {
    let mut values = headers.get_all(name).iter();
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn basic_credentials(headers: &HeaderMap) -> Option<(Vec<u8>, Vec<u8>)> {
    let value = single_value(headers, AUTHORIZATION.as_str())?;
    let bytes = value.as_bytes();
    if bytes.len() > MAX_BASIC_HEADER_BYTES {
        return None;
    }
    let space = bytes.iter().position(|byte| *byte == b' ')?;
    let (scheme, rest) = bytes.split_at(space);
    if !scheme.eq_ignore_ascii_case(BASIC_SCHEME) {
        return None;
    }
    let decoded = STANDARD.decode(rest.trim_ascii()).ok()?;
    let colon = decoded.iter().position(|byte| *byte == b':')?;
    let (user, from_colon) = decoded.split_at(colon);
    let password = from_colon.get(1..)?;
    Some((user.to_vec(), password.to_vec()))
}
