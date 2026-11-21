mod background;
mod composer;
mod credentials;
mod error;
mod forward;
mod notify;
mod public;
mod readiness;
mod rotate;
mod router;
mod schedules;
mod secrets;
mod signal;

use std::future::{Future, IntoFuture};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use whl_api::{ApiLimits, ApiSettings, ApiState, ComposerSettings, ForwardSettings};
use whl_auth::{AccessControl, AuthSettings, AuthState, DiscoveryBackoff};
use whl_capture::{CaptureSettings, CaptureState};
use whl_core::client_ip::TrustedProxies;
use whl_core::config::{AuthMode, Config};
use whl_core::limiter::{LimiterSettings, Limiters};
use whl_dns::{DnsServer, DnsSettings, StorageProbe};
use whl_forward::WorkerHandle;
use whl_live::{HUB_CAPACITY, Hub, ListenerHandle, LiveState, StreamSettings};
use whl_mail::{MailServer, MailSettings};
use whl_secrets::Keyring;
use whl_store::Store;
use whl_telemetry::series;

pub use background::{BackgroundSettings, LIMITER_CLEANUP_INTERVAL, STORAGE_REFRESH_INTERVAL};
pub use credentials::{StaleCredentials, stale_capture_credentials};
pub use error::ServerError;
pub use public::{
    HEADER_READ_TIMEOUT, HttpLimits, MAX_HEADER_COUNT, MAX_HEADER_LIST_BYTES, WRITE_TIMEOUT,
};
pub use rotate::{
    DEFAULT_BATCH_SIZE, MAX_BATCH_SIZE, RotateError, RotateProgress, RotateReport, rotate_secrets,
};
pub use secrets::{SecretsSetupError, keyring_from_config, rotation_keyring};
pub use signal::shutdown_signal;

use background::{Background, Workers};
use composer::CaptureDelivery;
use forward::ForwardRuntime;
use notify::NotifyRuntime;
use readiness::Readiness;
use schedules::ScheduleRuntime;

pub struct Bound {
    public: TcpListener,
    admin: TcpListener,
    public_addr: SocketAddr,
    admin_addr: SocketAddr,
    drain: Duration,
    grace: Duration,
    retention: Duration,
    api: ApiSettings,
    alias_quarantine: Duration,
    capture: CaptureSettings,
    forward: ForwardRuntime,
    composer: ComposerSettings,
    schedules: ScheduleRuntime,
    notify: NotifyRuntime,
    http: HttpLimits,
    auth: Option<AuthSettings>,
    auth_mode: AuthMode,
    proxies: TrustedProxies,
    limiters: Limiters,
    api_limits: ApiLimits,
    api_cors: bool,
    background: BackgroundSettings,
    secrets: Option<Arc<Keyring>>,
    dns: Option<DnsServer>,
    mail: Option<MailSettings>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsAddrs {
    pub udp: SocketAddr,
    pub tcp: SocketAddr,
}

impl Bound {
    pub fn public_addr(&self) -> SocketAddr {
        self.public_addr
    }

    pub fn admin_addr(&self) -> SocketAddr {
        self.admin_addr
    }

    pub fn dns_addrs(&self) -> Option<DnsAddrs> {
        self.dns.as_ref().map(|dns| DnsAddrs {
            udp: dns.udp_addr(),
            tcp: dns.tcp_addr(),
        })
    }

    pub fn with_http_limits(mut self, limits: HttpLimits) -> Self {
        self.http = limits;
        self
    }

    pub fn with_background(mut self, settings: BackgroundSettings) -> Self {
        self.background = settings;
        self
    }

    pub async fn serve(
        self,
        store: Store,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> Result<(), ServerError> {
        credentials::warn_unusable_capture_credentials(&store, self.secrets.as_deref()).await;
        let readiness = Arc::new(Readiness::default());
        let worker = self.forward.worker(&store)?;
        let notify_worker = self.notify.worker(&store)?;
        let auth = match self.auth.clone() {
            Some(settings) => {
                Some(AuthState::new(store.clone(), settings)?.with_proxies(self.proxies.clone()))
            }
            None => None,
        };
        let access = match &auth {
            Some(auth) => {
                readiness.set_auth_unavailable(true);
                metrics::gauge!(series::AUTH_AVAILABLE).set(0.0);
                match self.auth_mode {
                    AuthMode::Mixed => AccessControl::mixed(auth.clone(), self.retention),
                    AuthMode::Open | AuthMode::Sso => {
                        AccessControl::sso(auth.clone(), self.retention)
                    }
                }
            }
            None => AccessControl::open(),
        }
        .with_proxies(self.proxies.clone())
        .with_token_limits(self.limiters.clone());
        let capture =
            CaptureState::new(store.clone(), self.capture)?.with_limiters(self.limiters.clone());
        let capture = match &self.secrets {
            Some(keyring) => capture.with_secrets(Arc::clone(keyring)),
            None => capture,
        };
        let delivery = CaptureDelivery::new(whl_capture::router(capture.clone()));
        let state = ApiState::new(store.clone(), self.api.clone())
            .with_access(access)
            .with_limits(self.api_limits.clone())
            .with_alias_quarantine(self.alias_quarantine)
            .with_forward(ForwardSettings::with_system_lookup(self.forward.guard()))
            .with_composer(self.composer.with_local(Arc::new(delivery)))
            .with_schedules(self.schedules.settings())
            .with_notifications(self.notify.api_settings());
        let state = match self.notify.email_api() {
            Some(email) => state.with_email(email),
            None => state,
        };
        let state = match &self.secrets {
            Some(keyring) => state.with_secrets(Arc::clone(keyring)),
            None => state,
        };
        let authorizer = whl_api::stream_authorizer(state.access());
        let inbox = whl_api::inbox_selector(state.access(), store.clone(), self.retention);
        tracing::info!(public = %self.public_addr, admin = %self.admin_addr, "listening");

        let storage_full = capture.storage_full().clone();
        let schedule_runner =
            self.schedules
                .runner(&store, self.secrets.clone(), storage_full.clone())?;
        let mail = match self.mail {
            Some(settings) => {
                let flag = storage_full.clone();
                let settings = settings.with_storage_probe(Arc::new(move || flag.is_full()));
                Some(MailServer::bind(settings, store.clone()).await?)
            }
            None => None,
        };
        let (stop_tx, stop_rx) = watch::channel(false);
        let background_stop = stop_rx.clone();
        let hub = Hub::new(HUB_CAPACITY);
        let live = LiveState::new(
            store.clone(),
            Arc::clone(&hub),
            StreamSettings::new(self.retention)
                .with_instance(self.api.public_url.clone(), self.api.max_captures),
        )
        .with_authorizer(authorizer)
        .with_inbox(inbox);
        let listener = whl_live::spawn_listener(
            store.clone(),
            Arc::clone(&hub),
            until_stopped(stop_rx.clone()),
        );
        let liveness = listener.liveness();
        let (public_ready, public_started) = oneshot::channel();
        let public = serve_public(PublicParts {
            listener: self.public,
            live: live.clone(),
            api: state,
            capture,
            auth,
            readiness: Arc::clone(&readiness),
            limits: self.api_limits,
            api_cors: self.api_cors,
            http: self.http,
            stop: stop_rx.clone(),
            ready: public_ready,
        });
        let worker_stop = stop_rx.clone();
        let mail_stop = stop_rx.clone();
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
        if !liveness.is_alive() {
            return fail_listener(
                &readiness,
                &stop_tx,
                &live,
                &mut serving,
                self.grace,
                ListenerStop {
                    listener,
                    forward: None,
                    background: None,
                    mail: None,
                },
            )
            .await;
        }
        readiness.mark_ready();
        let dns = match self.dns {
            Some(server) => {
                let flag = storage_full.clone();
                let probe = StorageProbe::new(move || flag.is_full());
                Some(server.serve(store.clone(), probe, background_stop.clone())?)
            }
            None => None,
        };
        let background = Background::spawn(
            store.clone(),
            &self.background,
            storage_full,
            self.limiters,
            Workers {
                dns,
                schedules: schedule_runner,
                notify: notify_worker,
            },
            &background_stop,
        );
        let mut forward = worker.map(|worker| worker.spawn(until_stopped(worker_stop)));
        let mut mail_task = mail.map(|server| {
            tracing::info!(addr = %server.local_addr(), "the mail receiver is listening");
            tokio::spawn(server.serve(mail_stop))
        });

        tokio::select! {
            () = &mut shutdown => {}
            outcome = &mut serving => {
                let _ = stop_tx.send(true);
                live.close_streams();
                if let Some(forward) = forward.as_mut() {
                    forward.join().await;
                }
                join_mail(mail_task.take()).await;
                background.join().await;
                listener.join().await;
                return joined(outcome);
            }
            () = liveness.stopped() => {
                return fail_listener(
                    &readiness,
                    &stop_tx,
                    &live,
                    &mut serving,
                    self.grace,
                    ListenerStop {
                        listener,
                        forward: forward.as_mut(),
                        background: Some(background),
                        mail: mail_task.take(),
                    },
                )
                .await;
            }
        }

        readiness.mark_draining();
        tracing::info!(drain = ?self.drain, "shutdown started, draining");
        tokio::time::sleep(self.drain).await;

        let _ = stop_tx.send(true);
        live.close_streams();
        let stopping = async {
            let served = (&mut serving).await;
            if let Some(forward) = forward.as_mut() {
                forward.join().await;
            }
            served
        };
        let outcome = match tokio::time::timeout(self.grace, stopping).await {
            Ok(outcome) => joined(outcome),
            Err(_) => {
                tracing::warn!(grace = ?self.grace, "grace period elapsed, stopping with requests and forward attempts in flight");
                serving.abort();
                if let Some(forward) = &forward {
                    forward.abort();
                }
                Ok(())
            }
        };
        join_mail(mail_task.take()).await;
        background.join().await;
        listener.join().await;
        outcome
    }
}

pub async fn bind(config: &Config) -> Result<Bound, ServerError> {
    let secrets = keyring_from_config(config)?.map(Arc::new);
    match &secrets {
        Some(keyring) => tracing::info!(
            active_key = %keyring.active_id(),
            keys = keyring.key_ids().len(),
            "secrets are enabled"
        ),
        None => tracing::info!("secrets are disabled, no key set is configured"),
    }
    let (public, public_addr) = bind_listener("public", config.listen_addr).await?;
    let (admin, admin_addr) = bind_listener("admin", config.admin_addr).await?;
    let dns = match DnsSettings::from_config(config)? {
        Some(settings) => Some(DnsServer::bind(settings).await?),
        None => None,
    };
    let limiters = Limiters::new(&LimiterSettings::from_config(&config.rate))?;
    let proxies = TrustedProxies::new(config.trusted_proxies.clone());
    let api_limits = ApiLimits::new(limiters.clone(), proxies.clone());
    let forward = ForwardRuntime::from_config(config)?;
    let notify = NotifyRuntime::from_config(config, forward.guard(), secrets.clone())?;

    Ok(Bound {
        public,
        admin,
        public_addr,
        admin_addr,
        drain: config.shutdown.drain,
        grace: config.shutdown.grace,
        retention: config.capture.retention,
        api: ApiSettings::from_config(config),
        alias_quarantine: config.alias.quarantine,
        capture: CaptureSettings::from_config(config)?,
        forward,
        composer: ComposerSettings::from_config(config),
        schedules: ScheduleRuntime::from_config(config)?,
        notify,
        http: HttpLimits::default(),
        auth: AuthSettings::from_config(config)?,
        auth_mode: config.auth.mode,
        proxies,
        limiters,
        api_limits,
        api_cors: config.api_cors,
        background: BackgroundSettings::from_config(config),
        secrets,
        dns,
        mail: MailSettings::from_config(config)?,
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
    auth: Option<AuthState>,
    readiness: Arc<Readiness>,
    limits: ApiLimits,
    api_cors: bool,
    http: HttpLimits,
    stop: watch::Receiver<bool>,
    ready: oneshot::Sender<()>,
}

async fn serve_public(parts: PublicParts) -> Result<(), ServerError> {
    let discovery = parts.auth.clone().map(|auth| {
        tokio::spawn(discover_auth(
            auth,
            parts.stop.clone(),
            Arc::clone(&parts.readiness),
        ))
    });
    let router = router::public(
        parts.live,
        parts.api,
        parts.capture,
        parts.auth,
        parts.limits,
        parts.api_cors,
    );
    let _ = parts.ready.send(());
    let server = public::serve(
        parts.listener,
        router,
        parts.http,
        until_stopped(parts.stop),
    );
    let served = serve_listener("public", server).await;
    if let Some(discovery) = discovery {
        discovery.abort();
        if let Err(error) = discovery.await
            && !error.is_cancelled()
        {
            tracing::error!(%error, "the OIDC discovery task ended abnormally");
        }
    }
    served
}

async fn discover_auth(auth: AuthState, stop: watch::Receiver<bool>, readiness: Arc<Readiness>) {
    let mut backoff = DiscoveryBackoff::new();
    loop {
        let attempt = tokio::select! {
            () = until_stopped(stop.clone()) => return,
            attempt = auth.establish() => attempt,
        };
        match attempt {
            Ok(()) => {
                readiness.set_auth_unavailable(false);
                metrics::gauge!(series::AUTH_AVAILABLE).set(1.0);
                tracing::info!("sign-in is available");
                return;
            }
            Err(error) => {
                let delay = backoff.next_delay();
                tracing::warn!(%error, retry_in = ?delay, "sign-in is unavailable, the identity provider could not be reached");
                tokio::select! {
                    () = until_stopped(stop.clone()) => return,
                    () = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
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

struct ListenerStop<'a> {
    listener: ListenerHandle,
    forward: Option<&'a mut WorkerHandle>,
    background: Option<Background>,
    mail: Option<JoinHandle<()>>,
}

async fn join_mail(task: Option<JoinHandle<()>>) {
    if let Some(task) = task
        && let Err(error) = task.await
    {
        tracing::error!(%error, "the mail receiver ended abnormally");
    }
}

async fn fail_listener(
    readiness: &Readiness,
    stop: &watch::Sender<bool>,
    live: &LiveState,
    serving: &mut JoinHandle<Result<(), ServerError>>,
    grace: Duration,
    stopping: ListenerStop<'_>,
) -> Result<(), ServerError> {
    let ListenerStop {
        listener,
        mut forward,
        background,
        mail,
    } = stopping;
    tracing::error!(
        "the notification listener stopped, live updates cannot be delivered, shutting down"
    );
    readiness.mark_draining();
    let _ = stop.send(true);
    live.close_streams();
    let draining = async {
        let _ = (&mut *serving).await;
        if let Some(forward) = forward.as_deref_mut() {
            forward.join().await;
        }
    };
    if tokio::time::timeout(grace, draining).await.is_err() {
        tracing::warn!(
            ?grace,
            "grace period elapsed, stopping with requests in flight"
        );
        serving.abort();
        if let Some(forward) = forward.as_deref() {
            forward.abort();
        }
    }
    join_mail(mail).await;
    if let Some(background) = background {
        background.join().await;
    }
    listener.join().await;
    Err(ServerError::Serve {
        listener: "live",
        source: io::Error::other("the notification listener stopped"),
    })
}

pub(crate) async fn until_stopped(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

fn joined(
    outcome: Result<Result<(), ServerError>, tokio::task::JoinError>,
) -> Result<(), ServerError> {
    outcome.map_err(ServerError::Join)?
}
