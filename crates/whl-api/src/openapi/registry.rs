use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::quota::UsageDoc::openapi(),
        crate::rules::RulesDoc::openapi(),
        crate::secrets::SecretsDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
        crate::vars::VarsDoc::openapi(),
    ]
}
