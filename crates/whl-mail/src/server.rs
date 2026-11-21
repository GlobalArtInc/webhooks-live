use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;
use whl_core::proxy_protocol::{PREFIX_LEN, ProxyError, ProxyHeader, header_len, parse};
use whl_store::Store;
use whl_telemetry::series;

use crate::error::MailSetupError;
use crate::session::{Flow, SessionState, converse};
use crate::settings::MailSettings;
use crate::shared::{Shared, refused};
use crate::tls::{RELOAD_INTERVAL, TlsState};
use crate::wire::{Wire, until_stopped};

const DRAIN_GRACE: Duration = Duration::from_secs(5);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);
const BUSY_REPLY: &[u8] = b"421 4.3.2 Too many connections, try again later\r\n";
const RATE_REPLY: &[u8] = b"421 4.7.0 Too many connections from your address\r\n";

#[derive(Debug, thiserror::Error)]
enum ProxyReadError {
    #[error("cannot read the PROXY header: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Invalid(#[from] ProxyError),
}

pub struct MailServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    shared: Arc<Shared>,
}

struct SessionGauge;

impl SessionGauge {
    fn enter() -> Self {
        metrics::gauge!(series::MAIL_SESSIONS).increment(1.0);
        Self
    }
}

impl Drop for SessionGauge {
    fn drop(&mut self) {
        metrics::gauge!(series::MAIL_SESSIONS).decrement(1.0);
    }
}

impl MailServer {
    pub async fn bind(settings: MailSettings, store: Store) -> Result<Self, MailSetupError> {
        settings.limits.validate()?;
        let addr = settings.listen_addr;
        let tls = match settings.tls.clone() {
            Some(files) => Some(TlsState::load(files).await?),
            None => None,
        };
        let shared = Shared::new(store, settings, tls).map_err(|_| MailSetupError::Limits)?;
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|source| MailSetupError::Bind { addr, source })?;
        let local_addr = listener
            .local_addr()
            .map_err(|source| MailSetupError::Bind { addr, source })?;
        Ok(Self {
            listener,
            local_addr,
            shared: Arc::new(shared),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn serve(self, mut stop: watch::Receiver<bool>) {
        let Self {
            listener, shared, ..
        } = self;
        let reloader = shared
            .tls
            .clone()
            .map(|tls| tokio::spawn(reload_loop(tls, stop.clone())));
        let mut sessions = JoinSet::new();
        let mut cleanup = tokio::time::interval(CLEANUP_INTERVAL);
        loop {
            tokio::select! {
                biased;
                () = until_stopped(&mut stop) => break,
                _ = cleanup.tick() => shared.cleanup(),
                Some(finished) = sessions.join_next(), if !sessions.is_empty() => {
                    if let Err(error) = finished
                        && error.is_panic()
                    {
                        tracing::error!(%error, "a mail session task panicked");
                    }
                }
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer)) => admit(&shared, stream, peer, &mut sessions, &stop),
                    Err(error) => {
                        tracing::warn!(%error, "accepting a mail connection failed");
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                    }
                },
            }
        }
        drop(listener);
        let draining = async { while sessions.join_next().await.is_some() {} };
        if tokio::time::timeout(DRAIN_GRACE, draining).await.is_err() {
            tracing::warn!(
                open = sessions.len(),
                "mail sessions were still open after the drain grace"
            );
            sessions.abort_all();
        }
        if let Some(reloader) = reloader {
            reloader.abort();
        }
    }
}

fn admit(
    shared: &Arc<Shared>,
    stream: TcpStream,
    peer: SocketAddr,
    sessions: &mut JoinSet<()>,
    stop: &watch::Receiver<bool>,
) {
    let Ok(permit) = Arc::clone(&shared.sessions).try_acquire_owned() else {
        refused("too_busy");
        let _ = stream.try_write(BUSY_REPLY);
        return;
    };
    if let Err(error) = stream.set_nodelay(true) {
        tracing::debug!(%error, "cannot set TCP_NODELAY on a mail connection");
    }
    sessions.spawn(handle_connection(
        Arc::clone(shared),
        stream,
        peer,
        permit,
        stop.clone(),
    ));
}

async fn reload_loop(tls: Arc<TlsState>, mut stop: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval(RELOAD_INTERVAL);
    ticker.tick().await;
    loop {
        tokio::select! {
            () = until_stopped(&mut stop) => return,
            _ = ticker.tick() => {
                if let Err(error) = tls.reload().await {
                    tracing::warn!(%error, "reloading the mail certificate failed, keeping the previous one");
                }
            }
        }
    }
}

async fn read_proxy_header(stream: &mut TcpStream) -> Result<ProxyHeader, ProxyReadError> {
    let mut bytes = vec![0_u8; PREFIX_LEN];
    stream.read_exact(&mut bytes).await?;
    let total = header_len(&bytes)?;
    bytes.resize(total, 0);
    stream.read_exact(&mut bytes[PREFIX_LEN..]).await?;
    Ok(parse(&bytes)?.header)
}

async fn identify(shared: &Shared, stream: &mut TcpStream, peer: SocketAddr) -> Option<IpAddr> {
    if !shared.expects_proxy_header() {
        return Some(peer.ip().to_canonical());
    }
    if !shared.trusts(peer.ip()) {
        refused("untrusted_peer");
        return None;
    }
    match tokio::time::timeout(shared.limits.proxy_timeout, read_proxy_header(stream)).await {
        Ok(Ok(ProxyHeader::Proxied(addresses))) if !addresses.source.ip().is_unspecified() => {
            Some(addresses.source.ip().to_canonical())
        }
        Ok(Ok(ProxyHeader::Proxied(_))) => {
            refused("bad_proxy_header");
            tracing::warn!(%peer, "a trusted peer sent an unspecified source address");
            None
        }
        Ok(Ok(ProxyHeader::Local)) => {
            refused("proxy_local");
            None
        }
        Ok(Err(error)) => {
            refused("bad_proxy_header");
            tracing::warn!(%peer, %error, "a trusted peer sent an unusable PROXY header");
            None
        }
        Err(_) => {
            refused("proxy_timeout");
            None
        }
    }
}

async fn refuse_connection(stream: &mut TcpStream, reply: &[u8], timeout: Duration) {
    use tokio::io::AsyncWriteExt;
    let _ = tokio::time::timeout(timeout, stream.write_all(reply)).await;
}

async fn handle_connection(
    shared: Arc<Shared>,
    mut stream: TcpStream,
    peer: SocketAddr,
    permit: OwnedSemaphorePermit,
    stop: watch::Receiver<bool>,
) {
    let _permit = permit;
    let Some(client) = identify(&shared, &mut stream, peer).await else {
        return;
    };
    let write_timeout = shared.limits.write_timeout;
    if !shared.connection_allowed(client) {
        refused("connection_rate");
        refuse_connection(&mut stream, RATE_REPLY, write_timeout).await;
        return;
    }
    let Some(_ip_guard) = shared.ip_sessions.acquire(client) else {
        refused("ip_limit");
        refuse_connection(&mut stream, BUSY_REPLY, write_timeout).await;
        return;
    };
    let _gauge = SessionGauge::enter();

    let mut state = SessionState::new(Instant::now() + shared.limits.session_timeout);
    let mut wire = Wire::new(stream, stop.clone());
    let greeting = format!("220 {} ESMTP ready\r\n", shared.domain.unpack());
    if wire
        .write(greeting.as_bytes(), write_timeout)
        .await
        .is_err()
    {
        return;
    }
    match converse(&shared, &mut wire, &mut state, client, false).await {
        Flow::Finished => wire.close().await,
        Flow::Upgrade => upgrade(&shared, wire, state, client, stop).await,
    }
}

async fn upgrade(
    shared: &Shared,
    wire: Wire<TcpStream>,
    mut state: SessionState,
    client: IpAddr,
    stop: watch::Receiver<bool>,
) {
    let Some(tls) = shared.tls.as_ref() else {
        return;
    };
    let acceptor = tls.acceptor();
    let handshake = acceptor.accept(wire.into_stream());
    let stream = match tokio::time::timeout(shared.limits.tls_timeout, handshake).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(error)) => {
            refused("tls_handshake");
            tracing::debug!(%error, "a mail TLS handshake failed");
            return;
        }
        Err(_) => {
            refused("tls_timeout");
            return;
        }
    };
    let mut wire = Wire::new(stream, stop);
    state.restart_after_tls();
    converse(shared, &mut wire, &mut state, client, true).await;
    wire.close().await;
}
