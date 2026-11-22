use utoipa::OpenApi;

use super::dto::{
    ChannelDto, ChannelInputDoc, ChannelKindDoc, ChannelListDto, DeliveryDto, DeliveryKindDoc,
    DeliveryStateDoc, TriggerDoc,
};
use super::email::{EmailAddressDto, EmailAddressInputDoc, EmailAddressListDto, LinkTokenInputDoc};
use super::{email, handlers};

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::list,
        handlers::create,
        handlers::update,
        handlers::delete,
        handlers::test,
        email::list,
        email::add,
        email::remove,
        email::verify,
        email::disable
    ),
    components(schemas(
        ChannelDto,
        ChannelListDto,
        ChannelInputDoc,
        ChannelKindDoc,
        TriggerDoc,
        DeliveryDto,
        DeliveryKindDoc,
        DeliveryStateDoc,
        EmailAddressDto,
        EmailAddressListDto,
        EmailAddressInputDoc,
        LinkTokenInputDoc
    )),
    tags((name = "Notifications", description = "Notification channels of an endpoint (webhook, Slack, Discord, Telegram, e-mail, web push), their test sends, and the verified e-mail addresses that receive e-mail channels"))
)]
pub struct NotificationsDoc;
