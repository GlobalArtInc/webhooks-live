#![allow(dead_code)]

use utoipa::IntoParams;

#[derive(IntoParams)]
#[into_params(parameter_in = Path)]
pub struct EndpointIdParam {
    #[param(
        example = "7k2m9q4xw8c1d5f3h6j0n2p4",
        min_length = 24,
        max_length = 24,
        pattern = "^[0-9a-hjkmnp-tv-z]{24}$"
    )]
    id: String,
}
