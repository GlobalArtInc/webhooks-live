use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use whl_core::client_ip::TrustedProxies;
use whl_core::proxy_protocol::{self, PREFIX_LEN};
use whl_store::Store;

use crate::error::DnsError;
use crate::ingress::{datagram_ingress, resolver_of};
use crate::limits::Admission;
use crate::limits::ConnectionTable;
use crate::query::{HEADER_BYTES, MAX_QUERY_BYTES, Parsed, parse};
use crate::responder::{Outcome, Resolver, Responder, StorageProbe, Transport, count};
use crate::settings::{DnsLimits, DnsSettings, SWEEP_INTERVAL};

const RECEIVE_BYTES: usize = 2048;
const MAX_DATAGRAM_BYTES: usize = MAX_QUERY_BYTES + proxy_protocol::MAX_HEADER_LEN;
const ACCEPT_ERROR_PAUSE: Duration = Duration::from_millis(50);
const SEND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct DnsServer {
    udp: UdpSocket,
    tcp: TcpListener,
    udp_addr: SocketAddr,
    tcp_addr: SocketAddr,
    settings: DnsSettings,
}

#[derive(Debug)]
pub struct DnsHandle {
    responder: Arc<Responder>,
    tasks: Vec<JoinHandle<()>>,
}

impl DnsHandle {
    pub fn responder(&self) -> &Arc<Responder> {
        &self.responder
    }

    pub async fn join(self) {
        for task in self.tasks {
            if let Err(error) = task.await {
                tracing::error!(%error, "a DNS task ended abnormally");
            }
        }
    }
}

impl DnsServer {
    pub async fn bind(settings: DnsSettings) -> Result<Self, DnsError> {
        let addr = settings.listen_addr;
        let udp = UdpSocket::bind(addr)
            .await
            .map_err(|source| DnsError::Bind {
                transport: "udp",
                addr,
                source,
            })?;
        let tcp = TcpListener::bind(addr)
            .await
            .map_err(|source| DnsError::Bind {
                transport: "tcp",
                addr,
                source,
            })?;
        let local = |result: std::io::Result<SocketAddr>, transport| {
            result.map_err(|source| DnsError::Bind {
                transport,
                addr,
                source,
            })
        };
        Ok(Self {
            udp_addr: local(udp.local_addr(), "udp")?,
            tcp_addr: local(tcp.local_addr(), "tcp")?,
            udp,
            tcp,
            settings,
        })
    }

    pub fn udp_addr(&self) -> SocketAddr {
        self.udp_addr
    }

    pub fn tcp_addr(&self) -> SocketAddr {
        self.tcp_addr
    }

    pub fn serve(
        self,
        store: Store,
        storage_full: StorageProbe,
        stop: watch::Receiver<bool>,
    ) -> Result<DnsHandle, DnsError> {
        let responder = Arc::new(Responder::new(&self.settings, store, storage_full)?);
        let trusted = Arc::new(self.settings.trusted_proxies.clone());
        let limits = self.settings.limits;
        let tasks = vec![
            tokio::spawn(sweep_loop(Arc::clone(&responder), stopped(stop.clone()))),
            tokio::spawn(udp_loop(
                Arc::new(self.udp),
                Arc::clone(&responder),
                Arc::clone(&trusted),
                limits,
                stopped(stop.clone()),
            )),
            tokio::spawn(tcp_loop(
                self.tcp,
                Arc::clone(&responder),
                trusted,
                limits,
                stopped(stop),
            )),
        ];
        tracing::info!(udp = %self.udp_addr, tcp = %self.tcp_addr, "DNS capture is listening");
        Ok(DnsHandle { responder, tasks })
    }
}

async fn stopped(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|value| *value).await;
}

async fn sweep_loop(responder: Arc<Responder>, stop: impl Future<Output = ()>) {
    tokio::pin!(stop);
    loop {
        tokio::select! {
            () = &mut stop => return,
            () = tokio::time::sleep(SWEEP_INTERVAL) => responder.sweep(),
        }
    }
}

async fn udp_loop(
    socket: Arc<UdpSocket>,
    responder: Arc<Responder>,
    trusted: Arc<TrustedProxies>,
    limits: DnsLimits,
    stop: impl Future<Output = ()>,
) {
    tokio::pin!(stop);
    let permits = Arc::new(Semaphore::new(limits.max_concurrent_queries));
    let mut buffer = vec![0_u8; RECEIVE_BYTES];
    let mut handlers = JoinSet::new();
    loop {
        let received = tokio::select! {
            () = &mut stop => break,
            Some(_) = handlers.join_next(), if !handlers.is_empty() => continue,
            received = socket.recv_from(&mut buffer) => received,
        };
        let (length, peer) = match received {
            Ok(received) => received,
            Err(error) => {
                tracing::warn!(%error, "receiving a DNS datagram failed");
                tokio::time::sleep(ACCEPT_ERROR_PAUSE).await;
                continue;
            }
        };
        if length > MAX_DATAGRAM_BYTES {
            count(Outcome::Dropped, Transport::Udp);
            continue;
        }
        let ingress = match datagram_ingress(&trusted, peer, &buffer[..length]) {
            Ok(ingress) => ingress,
            Err(error) => {
                tracing::debug!(%error, %peer, "a DNS datagram was dropped at ingress");
                count(Outcome::Dropped, Transport::Udp);
                continue;
            }
        };
        if ingress.query.len() > MAX_QUERY_BYTES {
            count(Outcome::Dropped, Transport::Udp);
            continue;
        }
        let parsed = parse(ingress.query);
        if parsed == Parsed::Drop {
            count(Outcome::Dropped, Transport::Udp);
            continue;
        }
        let admission = responder.admit_datagram(ingress.resolver);
        if admission == Admission::Drop {
            count(Outcome::RateLimited, Transport::Udp);
            continue;
        }
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            count(Outcome::Dropped, Transport::Udp);
            continue;
        };
        let resolver = Resolver {
            address: ingress.resolver,
            transport: Transport::Udp,
        };
        let responder = Arc::clone(&responder);
        let socket = Arc::clone(&socket);
        handlers.spawn(async move {
            let _permit = permit;
            let slip = admission == Admission::Slip;
            let Some(bytes) = responder.respond(parsed, resolver, slip).await else {
                return;
            };
            match tokio::time::timeout(SEND_TIMEOUT, socket.send_to(&bytes, peer)).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::debug!(%error, %peer, "sending a DNS response failed"),
                Err(_) => tracing::debug!(%peer, "sending a DNS response timed out"),
            }
        });
    }
    handlers.shutdown().await;
}

async fn tcp_loop(
    listener: TcpListener,
    responder: Arc<Responder>,
    trusted: Arc<TrustedProxies>,
    limits: DnsLimits,
    stop: impl Future<Output = ()>,
) {
    tokio::pin!(stop);
    let permits = Arc::new(Semaphore::new(limits.max_connections));
    let connections = ConnectionTable::new(limits.tcp_per_address);
    let mut sessions = JoinSet::new();
    loop {
        let accepted = tokio::select! {
            () = &mut stop => break,
            Some(_) = sessions.join_next(), if !sessions.is_empty() => continue,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "accepting a DNS connection failed");
                tokio::time::sleep(ACCEPT_ERROR_PAUSE).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            count(Outcome::Dropped, Transport::Tcp);
            continue;
        };
        let responder = Arc::clone(&responder);
        let trusted = Arc::clone(&trusted);
        let connections = Arc::clone(&connections);
        sessions.spawn(async move {
            let _permit = permit;
            let lifetime = limits.tcp_lifetime;
            let session =
                serve_connection(stream, peer, &responder, &trusted, &connections, limits);
            if tokio::time::timeout(lifetime, session).await.is_err() {
                tracing::debug!(%peer, "a DNS connection reached its lifetime limit");
            }
        });
    }
    sessions.shutdown().await;
}

async fn serve_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    responder: &Responder,
    trusted: &TrustedProxies,
    connections: &Arc<ConnectionTable>,
    limits: DnsLimits,
) {
    let address = if trusted.contains(peer.ip()) {
        match read_proxy_header(&mut stream, peer.ip(), limits.tcp_read_timeout).await {
            Some(address) => address,
            None => {
                count(Outcome::Dropped, Transport::Tcp);
                return;
            }
        }
    } else {
        peer.ip().to_canonical()
    };
    let Some(_guard) = connections.enter(address) else {
        count(Outcome::RateLimited, Transport::Tcp);
        return;
    };
    let resolver = Resolver {
        address,
        transport: Transport::Tcp,
    };
    for _ in 0..limits.tcp_max_queries {
        let mut prefix = [0_u8; 2];
        if read_within(&mut stream, &mut prefix, limits.tcp_read_timeout)
            .await
            .is_none()
        {
            return;
        }
        let length = usize::from(u16::from_be_bytes(prefix));
        if !(HEADER_BYTES..=MAX_QUERY_BYTES).contains(&length) {
            count(Outcome::Dropped, Transport::Tcp);
            return;
        }
        let mut query = vec![0_u8; length];
        if read_within(&mut stream, &mut query, limits.tcp_read_timeout)
            .await
            .is_none()
        {
            return;
        }
        let parsed = parse(&query);
        if parsed == Parsed::Drop {
            count(Outcome::Dropped, Transport::Tcp);
            return;
        }
        if responder.admit_stream() != Admission::Answer {
            count(Outcome::RateLimited, Transport::Tcp);
            return;
        }
        let Some(bytes) = responder.respond(parsed, resolver, false).await else {
            return;
        };
        let Ok(framed_length) = u16::try_from(bytes.len()) else {
            return;
        };
        let mut framed = Vec::with_capacity(bytes.len() + 2);
        framed.extend_from_slice(&framed_length.to_be_bytes());
        framed.extend_from_slice(&bytes);
        match tokio::time::timeout(limits.tcp_write_timeout, stream.write_all(&framed)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::debug!(%error, %peer, "writing a DNS response failed");
                return;
            }
            Err(_) => return,
        }
    }
}

async fn read_within(stream: &mut TcpStream, buffer: &mut [u8], timeout: Duration) -> Option<()> {
    match tokio::time::timeout(timeout, stream.read_exact(buffer)).await {
        Ok(Ok(_)) => Some(()),
        Ok(Err(_)) | Err(_) => None,
    }
}

async fn read_proxy_header(
    stream: &mut TcpStream,
    peer: IpAddr,
    timeout: Duration,
) -> Option<IpAddr> {
    let mut header = vec![0_u8; PREFIX_LEN];
    read_within(stream, &mut header, timeout).await?;
    let total = proxy_protocol::header_len(&header).ok()?;
    header.resize(total, 0);
    read_within(stream, &mut header[PREFIX_LEN..], timeout).await?;
    let parsed = proxy_protocol::parse(&header).ok()?;
    Some(resolver_of(parsed.header, peer))
}
