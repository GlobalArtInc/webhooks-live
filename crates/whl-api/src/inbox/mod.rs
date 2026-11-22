mod cursor;
mod doc;
mod list;
mod selection;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Query;
use axum::http::Uri;
use axum::response::IntoResponse;
use serde::Deserialize;
use whl_auth::AccessControl;
use whl_live::{InboxSelection, InboxSelector, SelectionFuture, StreamRequest};
use whl_store::Store;

use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::routing::{Routes, get};

pub use doc::InboxDoc;

pub fn routes() -> Routes {
    Routes::new().route("/inbox/captures", get(list::list))
}

#[derive(Debug, Default, Deserialize)]
struct SelectionParams {
    endpoints: Option<String>,
    folder: Option<String>,
}

#[derive(Debug)]
struct RoleSelector {
    access: AccessControl,
    store: Store,
    retention: Duration,
}

impl InboxSelector for RoleSelector {
    fn select<'a>(&'a self, request: &'a StreamRequest, uri: &'a Uri) -> SelectionFuture<'a> {
        Box::pin(async move {
            let params = Query::<SelectionParams>::try_from_uri(uri)
                .map_err(|_| {
                    ApiError::Validation(vec![FieldViolation::new(
                        "query",
                        FieldCode::InvalidFormat,
                        "the query string is malformed or repeats a parameter",
                    )])
                    .into_response()
                })?
                .0;
            let caller = self
                .access
                .caller(request.headers(), request.peer())
                .await
                .map_err(IntoResponse::into_response)?;
            let endpoints = selection::select(
                &self.access,
                &self.store,
                self.retention,
                &caller,
                selection::SelectionRequest {
                    endpoints: params.endpoints.as_deref(),
                    folder: params.folder.as_deref(),
                    resumed: None,
                    headers: request.headers(),
                    grant_query: uri.query(),
                },
            )
            .await
            .map_err(IntoResponse::into_response)?;
            Ok(InboxSelection {
                endpoints,
                user: caller.user().map(|user| user.id.to_string()),
                address: caller.ip(),
            })
        })
    }
}

pub fn inbox_selector(
    access: &AccessControl,
    store: Store,
    retention: Duration,
) -> Arc<dyn InboxSelector> {
    Arc::new(RoleSelector {
        access: access.clone(),
        store,
        retention,
    })
}
