use utoipa::OpenApi;

use super::dto::{ForwardRuleInputDoc, RuleDto, RuleListDto};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::list, handlers::create, handlers::update, handlers::delete),
    components(schemas(RuleDto, RuleListDto, ForwardRuleInputDoc)),
    tags((name = "Forwarding", description = "Forward rules: send the captures of an endpoint on to a target URL"))
)]
pub struct ForwardDoc;
