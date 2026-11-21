mod error;
mod public;
mod readiness;
mod router;
mod signal;

use std::future::{Future, IntoFuture};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use whl_api::{ApiSettings, ApiState};
use whl_auth::{AuthSettings, AuthState};
use whl_capture::{CaptureSettings, CaptureState};
use whl_core::config::Config;
use whl_live::{HUB_CAPACITY, Hub, LiveState, StreamSettings};
use whl_store::Store;

pub use error::ServerError;
pub use public::{HEADER_READ_TIMEOUT, HttpLimits, MAX_HEADER_COUNT, MAX_HEADER_LIST_BYTES};
pub use signal::shutdown_signal;

use readiness::Readiness;

pub struct Bound {
    public: TcpListener,
    admin: TcpListener,
    public_addr: SocketAddr,
    admin_addr: SocketAddr,
    drain: Duration,
    grace: Duration,
    retention: Duration,
    api: ApiSettings,
    capture: CaptureSettings,
    http: HttpLimits,
    auth: Option<AuthSettings>,
}

impl Bound {
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr
    }

    pub fn admin_addr(&self) -> SocketAddr {
        self.admin_addr
    }

    pub fn with_http_limits(mut self, limits: HttpLimits) -> Self {
        self.http = limits;
        self
    }

    pub async fn serve(
        self,
        store: Store,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), ServerError> {
        let readiness = Arc::new(Readiness::default());
        let state = ApiState::new(store.clone(), self.api.clone());
        tracing::info!(public = %self.public_addr, admin = %self.admin_addr, "listening");

        let capture = CaptureState::new(store.clone(), self.capture)?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let hub = Hub::new(HUB_CAPACITY);
        let live = LiveState::new(
            store.clone(),
            Arc::clone(&hub),
            StreamSettings::new(self.retention),
        );
        let listener = whl_live::spawn_listener(
            store.clone(),
            Arc::clone(&hub),
            until_stopped(stop_rx.clone()),
        );
        let (public_ready, public_started) = oneshot::channel();
        let public = serve_public(PublicParts {
            listener: self.public,
            live: live.clone(),
            api: state,
            capture,
            store: store.clone(),
            auth: self.auth.clone(),
            http: self.http,
            stop: stop_rx.clone(),
            ready: public_ready,
        });
        let admin = axum::serve(self.admin, router::admin(Arc::clone(&readiness)))
            .with_graceful_shutdown(until_stopped(stop_rx));

        let mut serving = tokio::spawn(async move {
            tokio::try_join!(public, serve_listener("admin", admin),).map(|_| ())
        });

        tokio::pin!(shutdown);
        let startup = async {
            let schema = store.check_schema_version().await;
            if schema.is_ok() && public_started.await.is_err() {
                std::future::pending::<()>().await;
            }
            schema
        };
        let checked = tokio::select! {
            outcome = startup => outcome,
            outcome = &mut serving => return joined(outcome),
            () = &mut shutdown => {
                let _ = stop_tx.send(true);
                live.close_streams();
                let outcome = joined(serving.await);
                listener.join().await;
                return outcome;
            }
        };
        if let Err(error) = checked {
            let _ = stop_tx.send(true);
            live.close_streams();
            let _ = serving.await;
            listener.join().await;
            return Err(error.into());
        }
        readiness.mark_ready();

        tokio::select! {
            () = &mut shutdown => {}
            outcome = &mut serving => return joined(outcome),
        }

        readiness.mark_draining();
        tracing::info!(drain = ?self.drain, "shutdown started, draining");
        tokio::time::sleep(self.drain).await;

        let _ = stop_tx.send(true);
        live.close_streams();
        let outcome = match tokio::time::timeout(self.grace, &mut serving).await {
            Ok(outcome) => joined(outcome),
            Err(_) => {
                tracing::warn!(grace = ?self.grace, "grace period elapsed, stopping with requests in flight");
                serving.abort();
                Ok(())
            }
        };
        listener.join().await;
        outcome
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
        retention: config.capture.retention,
        api: ApiSettings::from_config(config),
        capture: CaptureSettings::from_config(config)?,
        http: HttpLimits::default(),
        auth: AuthSettings::from_config(config)?,
    })
}

pub async fn run(
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ServerError> {
    let store = Store::connect(&config.database).await?;
    bind(&config).await?.serve(store, shutdown).await
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

struct PublicParts {
    listener: TcpListener,
    live: LiveState,
    api: ApiState,
    capture: CaptureState,
    store: Store,
    auth: Option<AuthSettings>,
    http: HttpLimits,
    stop: watch::Receiver<bool>,
    ready: oneshot::Sender<()>,
}

async fn serve_public(parts: PublicParts) -> Result<(), ServerError> {
    let auth = match parts.auth {
        Some(settings) => {
            let discovery = AuthState::discover(parts.store, settings);
            tokio::select! {
                state = discovery => Some(state?),
                () = until_stopped(parts.stop.clone()) => return Ok(()),
            }
        }
        None => None,
    };
    let router = router::public(parts.live, parts.api, parts.capture, auth);
    let _ = parts.ready.send(());
    let server = public::serve(
        parts.listener,
        router,
        parts.http,
        until_stopped(parts.stop),
    );
    serve_listener("public", server).await
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
