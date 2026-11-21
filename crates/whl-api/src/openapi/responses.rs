#![allow(dead_code)]

use utoipa::ToResponse;

use crate::error::ErrorBody;

#[derive(ToResponse)]
#[response(
    description = "`invalid_json`: the body is not a JSON object, or is empty where a body is required"
)]
pub struct BadRequest(ErrorBody);

#[derive(ToResponse)]
#[response(description = "`unauthenticated` or `invalid_token`: no valid session or Bearer token")]
pub struct Unauthenticated(ErrorBody);

#[derive(ToResponse)]
#[response(description = "`payload_too_large`: the request body is over its limit")]
pub struct PayloadTooLarge(ErrorBody);

#[derive(ToResponse)]
#[response(description = "`unsupported_media_type`: the body is not `application/json`")]
pub struct UnsupportedMediaType(ErrorBody);

#[derive(ToResponse)]
#[response(description = "`validation_failed`: `error.fields` lists every violation, up to 50")]
pub struct ValidationFailed(ErrorBody);

#[derive(ToResponse)]
#[response(
    description = "`rate_limited` or `quota_exceeded`: wait `Retry-After` seconds",
    headers(("Retry-After" = u64, description = "Seconds until the limit resets"))
)]
pub struct RateLimited(ErrorBody);

#[derive(ToResponse)]
#[response(
    description = "`store_unavailable`: the database timed out or is down",
    headers(("Retry-After" = u64, description = "Always 5"))
)]
pub struct StoreUnavailable(ErrorBody);

#[derive(ToResponse)]
#[response(
    description = "`internal_error`: an unexpected server failure; details only in server logs"
)]
pub struct InternalError(ErrorBody);
