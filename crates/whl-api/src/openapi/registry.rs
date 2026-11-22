use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::aliases::AliasesDoc::openapi(),
        crate::captures::CapturesDoc::openapi(),
        crate::cloning::CloningDoc::openapi(),
        crate::composer::ComposerDoc::openapi(),
        crate::export::ExportDoc::openapi(),
        crate::folders::FoldersDoc::openapi(),
        crate::forward::ForwardDoc::openapi(),
        crate::handlers::EndpointsDoc::openapi(),
        crate::import::ImportDoc::openapi(),
        crate::inbox::InboxDoc::openapi(),
        crate::members::MembersDoc::openapi(),
        crate::notify::NotificationsDoc::openapi(),
        crate::push::PushDoc::openapi(),
        crate::quota::UsageDoc::openapi(),
        crate::rules::RulesDoc::openapi(),
        crate::schedules::SchedulesDoc::openapi(),
        crate::script::ScriptDoc::openapi(),
        crate::secrets::SecretsDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::templating::TemplatingDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
        crate::vars::VarsDoc::openapi(),
    ]
}
