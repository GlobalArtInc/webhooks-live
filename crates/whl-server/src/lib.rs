mod error;
mod readiness;
mod router;
mod signal;

use std::future::{Future, IntoFuture};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::watch;
use whl_core::config::Config;

pub use error::ServerError;
pub use signal::shutdown_signal;

use readiness::Readiness;

pub struct Bound {
    public: TcpListener,
    admin: TcpListener,
    public_addr: SocketAddr,
    admin_addr: SocketAddr,
    drain: Duration,
    grace: Duration,
}

impl Bound {
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr
    }

    pub fn admin_addr(&self) -> SocketAddr {
        self.admin_addr
    }

    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), ServerError> {
        let readiness = Arc::new(Readiness::default());
        readiness.mark_ready();
        tracing::info!(public = %self.public_addr, admin = %self.admin_addr, "listening");

        let (stop_tx, stop_rx) = watch::channel(false);
        let public = axum::serve(self.public, router::public())
            .with_graceful_shutdown(until_stopped(stop_rx.clone()));
        let admin = axum::serve(self.admin, router::admin(Arc::clone(&readiness)))
            .with_graceful_shutdown(until_stopped(stop_rx));

        let mut serving = tokio::spawn(async move {
            tokio::try_join!(
                serve_listener("public", public),
                serve_listener("admin", admin),
            )
            .map(|_| ())
        });

        tokio::pin!(shutdown);
        tokio::select! {
            () = &mut shutdown => {}
            outcome = &mut serving => return joined(outcome),
        }

        readiness.mark_draining();
        tracing::info!(drain = ?self.drain, "shutdown started, draining");
        tokio::time::sleep(self.drain).await;

        let _ = stop_tx.send(true);
        match tokio::time::timeout(self.grace, &mut serving).await {
            Ok(outcome) => joined(outcome),
            Err(_) => {
                tracing::warn!(grace = ?self.grace, "grace period elapsed, stopping with requests in flight");
                serving.abort();
                Ok(())
            }
        }
    }
}

pub async fn bind(config: &Config) -> Result<Bound, ServerError> {
    let (public, public_addr) = bind_listener("public", config.listen_addr).await?;
    let (admin, admin_addr) = bind_listener("admin", config.admin_addr).await?;

    Ok(Bound {
        public,
        admin,
        public_addr,
        admin_addr,
        drain: config.shutdown.drain,
        grace: config.shutdown.grace,
    })
}

pub async fn run(
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    bind(&config).await?.serve(shutdown).await
}

async fn bind_listener(
    listener: &'static str,
    addr: SocketAddr,
) -> Result<(TcpListener, SocketAddr), ServerError> {
    let bind_error = |source| ServerError::Bind {
        listener,
        addr,
        source,
    };
    let socket = TcpListener::bind(addr).await.map_err(bind_error)?;
    let local = socket.local_addr().map_err(bind_error)?;
    Ok((socket, local))
}

async fn serve_listener(
    listener: &'static str,
    server: impl IntoFuture<Output = io::Result<()>>,
) -> Result<(), ServerError> {
    server
        .into_future()
        .await
        .map_err(|source| ServerError::Serve { listener, source })
}

async fn until_stopped(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

fn joined(
    outcome: Result<Result<(), ServerError>, tokio::task::JoinError>,
) -> Result<(), ServerError> {
    outcome.map_err(ServerError::Join)?
}
