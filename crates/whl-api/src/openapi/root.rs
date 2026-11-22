use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use super::responses::{
    BadRequest, InternalError, PayloadTooLarge, RateLimited, StoreUnavailable, Unauthenticated,
    UnauthenticatedOrGrantRequired, UnsupportedMediaType, ValidationFailed,
};
use super::serve;
use crate::error::{
    ErrorBody, ErrorDetail, FieldCode, FieldViolation, LimitDetail, ReferenceDetail,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "webhooks.live API",
        description = "Management API of a webhooks.live instance. Errors use the envelope of the `Error` schema; clients branch on `error.code`."
    ),
    servers((url = "/api/v1", description = "This instance")),
    tags((name = "Meta", description = "The API itself")),
    paths(serve::show),
    components(
        schemas(ErrorBody, ErrorDetail, FieldViolation, FieldCode, LimitDetail, ReferenceDetail),
        responses(
            BadRequest,
            Unauthenticated,
            UnauthenticatedOrGrantRequired,
            PayloadTooLarge,
            UnsupportedMediaType,
            ValidationFailed,
            RateLimited,
            StoreUnavailable,
            InternalError
        )
    ),
    modifiers(&SecuritySchemes)
)]
pub(super) struct RootDoc;

struct SecuritySchemes;

impl Modify for SecuritySchemes {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.info.license = None;
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session_cookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                "__Host-whl_session",
                "Browser session set by /auth/callback (`whl_session` over plain HTTP). Mutating requests must send an `Origin` equal to the public origin.",
            ))),
        );
        components.add_security_scheme(
            "api_token",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("whl_<52 base32 characters>")
                    .description(Some(
                        "Personal API token created with POST /tokens. With a Bearer header the cookie is ignored.",
                    ))
                    .build(),
            ),
        );
    }
}
