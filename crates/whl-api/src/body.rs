use axum::body::Bytes;
use axum::extract::{FromRequest, Request};
use axum::http::{HeaderValue, StatusCode, header};

use crate::error::ApiError;

pub const MAX_BODY_BYTES: usize = 256 * 1024;

pub struct RawBody(pub Bytes);

impl<S: Send + Sync> FromRequest<S> for RawBody {
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let content_type = request.headers().get(header::CONTENT_TYPE).cloned();
        let bytes = Bytes::from_request(request, state)
            .await
            .map_err(|rejection| {
                if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::PayloadTooLarge
                } else {
                    ApiError::InvalidJson("the request body could not be read")
                }
            })?;
        if !bytes.is_empty() && !is_json(content_type.as_ref()) {
            return Err(ApiError::UnsupportedMediaType);
        }
        Ok(Self(bytes))
    }
}

fn is_json(value: Option<&HeaderValue>) -> bool {
    value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}
