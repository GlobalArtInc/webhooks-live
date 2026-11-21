use std::io;
use std::net::SocketAddr;

use tokio::task::JoinError;
use whl_capture::StateError;
use whl_store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("cannot prepare the database: {0}")]
    Database(#[from] StoreError),
    #[error("cannot set up the capture path: {0}")]
    Capture(#[from] StateError),
    #[error("cannot bind the {listener} listener on {addr}: {source}")]
    Bind {
        listener: &'static str,
        addr: SocketAddr,
        source: io::Error,
    },
    #[error("the {listener} listener failed: {source}")]
    Serve {
        listener: &'static str,
        source: io::Error,
    },
    #[error("a server task failed to finish: {0}")]
    Join(#[from] JoinError),
}
