use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::aliases::AliasesDoc::openapi(),
        crate::handlers::EndpointsDoc::openapi(),
        crate::inbox::InboxDoc::openapi(),
        crate::members::MembersDoc::openapi(),
        crate::quota::UsageDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
    ]
}
