use std::time::Duration;

use axum::body::{Body, Bytes};
use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("the upload failed: {0}")]
    Body(#[source] axum::Error),
    #[error("the upload sent nothing for {0:?}")]
    Idle(Duration),
}

pub fn guarded(body: Body, idle: Duration) -> BoxStream<'static, Result<Bytes, UploadError>> {
    stream::unfold(body.into_data_stream(), move |mut data| async move {
        match tokio::time::timeout(idle, data.next()).await {
            Err(_) => Some((Err(UploadError::Idle(idle)), data)),
            Ok(None) => None,
            Ok(Some(Err(error))) => Some((Err(UploadError::Body(error)), data)),
            Ok(Some(Ok(chunk))) => Some((Ok(chunk), data)),
        }
    })
    .boxed()
}
