use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

pub(super) fn area_docs() -> Vec<Document> {
    vec![
        crate::notify::NotificationsDoc::openapi(),
        crate::push::PushDoc::openapi(),
        crate::quota::UsageDoc::openapi(),
        super::streams::StreamsDoc::openapi(),
        crate::tokens::TokensDoc::openapi(),
    ]
}
