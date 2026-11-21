use std::future::pending;

use tokio::signal::unix::{SignalKind, signal};

pub async fn shutdown_signal() {
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "cannot listen for SIGTERM");
                pending::<()>().await;
            }
        }
    };
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "cannot listen for SIGINT");
            pending::<()>().await;
        }
    };

    tokio::select! {
        () = terminate => {}
        () = interrupt => {}
    }
}
