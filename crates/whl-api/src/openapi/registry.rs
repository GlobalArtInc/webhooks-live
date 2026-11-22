use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::cloning::CloningDoc::openapi(),
        crate::folders::FoldersDoc::openapi(),
        crate::quota::UsageDoc::openapi(),
        crate::schedules::SchedulesDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
    ]
}
