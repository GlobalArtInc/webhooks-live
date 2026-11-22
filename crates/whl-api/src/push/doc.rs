use utoipa::OpenApi;

use super::dto::{
    CreatedDto, PublicKeyDto, PushSubscriptionInputDoc, PushSubscriptionKeysDoc, SubscriptionDto,
    SubscriptionListDto,
};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::public_key,
        handlers::list,
        handlers::create,
        handlers::delete
    ),
    components(schemas(
        PublicKeyDto,
        CreatedDto,
        SubscriptionDto,
        SubscriptionListDto,
        PushSubscriptionInputDoc,
        PushSubscriptionKeysDoc
    )),
    tags((name = "Push", description = "Web push subscriptions of the signed-in user and the instance's VAPID public key"))
)]
pub struct PushDoc;
