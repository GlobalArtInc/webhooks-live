use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo;
use axum::http::Request;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::{GracefulShutdown, Watcher};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tower::ServiceExt;

pub const MAX_HEADER_LIST_BYTES: u32 = 64 * 1024;
pub const MAX_HEADER_COUNT: usize = 100;
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

const MIN_HEADER_BUFFER: usize = 8192;
const ACCEPT_BACKOFF: Duration = Duration::from_secs(1);
const PREFACE_POLL: Duration = Duration::from_millis(20);
const H2_PREFACE: &[u8; 24] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpLimits {
    pub max_header_list_bytes: u32,
    pub max_header_count: usize,
    pub header_read_timeout: Duration,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_header_list_bytes: MAX_HEADER_LIST_BYTES,
            max_header_count: MAX_HEADER_COUNT,
            header_read_timeout: HEADER_READ_TIMEOUT,
        }
    }
}

impl HttpLimits {
    fn builder(&self) -> Builder<TokioExecutor> {
        let buffer = usize::try_from(self.max_header_list_bytes)
            .unwrap_or(usize::MAX)
            .max(MIN_HEADER_BUFFER);
        let mut builder = Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .max_buf_size(buffer)
            .max_headers(self.max_header_count)
            .header_read_timeout(self.header_read_timeout);
        builder
            .http2()
            .timer(TokioTimer::new())
            .max_header_list_size(self.max_header_list_bytes);
        builder
    }
}

pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    limits: HttpLimits,
    stop: impl Future<Output = ()>,
) -> io::Result<()> {
    let builder = limits.builder();
    let graceful = GracefulShutdown::new();
    let mut connections = JoinSet::new();
    tokio::pin!(stop);

    loop {
        tokio::select! {
            () = &mut stop => break,
            Some(finished) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = finished {
                    tracing::error!(%error, "a connection task failed");
                }
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    connections.spawn(serve_connection(
                        stream,
                        peer,
                        app.clone(),
                        builder.clone(),
                        graceful.watcher(),
                        limits.header_read_timeout,
                    ));
                }
                Err(error) => {
                    tracing::warn!(%error, "accepting a connection failed");
                    if !is_connection_error(&error) {
                        tokio::time::sleep(ACCEPT_BACKOFF).await;
                    }
                }
            },
        }
    }

    drop(listener);
    graceful.shutdown().await;
    Ok(())
}

async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: Router,
    builder: Builder<TokioExecutor>,
    watcher: Watcher,
    header_read_timeout: Duration,
) {
    match tokio::time::timeout(header_read_timeout, settle_protocol(&stream)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::debug!(%peer, %error, "the connection failed before the first request");
            return;
        }
        Err(_) => {
            tracing::debug!(%peer, "the first bytes did not arrive in time, closing");
            return;
        }
    }

    let service = service_fn(move |mut request: Request<Incoming>| {
        request.extensions_mut().insert(ConnectInfo(peer));
        app.clone().oneshot(request)
    });
    let connection = builder.serve_connection(TokioIo::new(stream), service);
    if let Err(error) = watcher.watch(connection).await {
        tracing::debug!(%peer, %error, "the connection ended with an error");
    }
}

async fn settle_protocol(stream: &TcpStream) -> io::Result<()> {
    let mut seen = [0_u8; H2_PREFACE.len()];
    loop {
        let read = stream.peek(&mut seen).await?;
        if read == 0 || read == seen.len() || seen[..read] != H2_PREFACE[..read] {
            return Ok(());
        }
        tokio::time::sleep(PREFACE_POLL).await;
    }
}

fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}
