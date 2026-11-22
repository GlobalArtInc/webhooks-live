use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::composer::ComposerDoc::openapi(),
        crate::export::ExportDoc::openapi(),
        crate::forward::ForwardDoc::openapi(),
        crate::import::ImportDoc::openapi(),
        crate::quota::UsageDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
    ]
}
