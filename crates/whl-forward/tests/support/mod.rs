#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use whl_forward::{GuardConfig, GuardedClient, OutboundRequest, StaticLookup, validate_target_url};

const FLOOD_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct SeenRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl SeenRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub enum Reply {
    Respond {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        delay: Duration,
    },
    Stall,
    StallAfterHead,
    Flood {
        total: usize,
    },
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self::Respond {
            status: 200,
            headers: vec![
                ("content-type".to_owned(), "text/plain".to_owned()),
                ("x-secret".to_owned(), "not-selected".to_owned()),
            ],
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    pub fn redirect(status: u16, location: &str) -> Self {
        Self::Respond {
            status,
            headers: vec![("location".to_owned(), location.to_owned())],
            body: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    pub fn after(self, wait: Duration) -> Self {
        match self {
            Self::Respond {
                status,
                headers,
                body,
                ..
            } => Self::Respond {
                status,
                headers,
                body,
                delay: wait,
            },
            other => other,
        }
    }
}

pub struct TestServer {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
    connections: Arc<AtomicUsize>,
    flooded: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl TestServer {
    pub async fn start(
        bind: &str,
        handler: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static,
    ) -> Self {
        Self::try_start(bind, handler)
            .await
            .unwrap_or_else(|error| panic!("cannot bind test server on {bind}: {error}"))
    }

    pub async fn start_if_bindable(
        bind: &str,
        handler: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static,
    ) -> Option<Self> {
        match Self::try_start(bind, handler).await {
            Ok(server) => Some(server),
            Err(error) => {
                eprintln!("skipping the case that needs a server on {bind}: cannot bind: {error}");
                None
            }
        }
    }

    async fn try_start(
        bind: &str,
        handler: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind).await?;
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let flooded = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(handler);
        let task = tokio::spawn({
            let seen = Arc::clone(&seen);
            let connections = Arc::clone(&connections);
            let flooded = Arc::clone(&flooded);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    connections.fetch_add(1, Ordering::SeqCst);
                    let seen = Arc::clone(&seen);
                    let flooded = Arc::clone(&flooded);
                    let handler = Arc::clone(&handler);
                    tokio::spawn(async move {
                        serve(stream, seen, flooded, handler).await;
                    });
                }
            }
        });
        Ok(Self {
            addr,
            seen,
            connections,
            flooded,
            task,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn seen(&self) -> Vec<SeenRequest> {
        self.seen.lock().unwrap().clone()
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn flooded_bytes(&self) -> usize {
        self.flooded.load(Ordering::SeqCst)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    mut stream: TcpStream,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
    flooded: Arc<AtomicUsize>,
    handler: Arc<impl Fn(&SeenRequest) -> Reply>,
) {
    loop {
        let Some(request) = read_request(&mut stream).await else {
            return;
        };
        seen.lock().unwrap().push(request.clone());
        match handler(&request) {
            Reply::Respond {
                status,
                headers,
                body,
                delay,
            } => {
                tokio::time::sleep(delay).await;
                let mut head = format!("HTTP/1.1 {status} Test\r\n");
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
                if stream.write_all(head.as_bytes()).await.is_err()
                    || stream.write_all(&body).await.is_err()
                {
                    return;
                }
            }
            Reply::Stall => std::future::pending::<()>().await,
            Reply::StallAfterHead => {
                let _ = stream
                    .write_all(b"HTTP/1.1 200 Test\r\ncontent-length: 100000\r\n\r\npartial")
                    .await;
                std::future::pending::<()>().await;
            }
            Reply::Flood { total } => {
                let head = format!("HTTP/1.1 200 Test\r\ncontent-length: {total}\r\n\r\n");
                if stream.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let chunk = vec![b'x'; FLOOD_CHUNK_BYTES];
                let mut sent = 0;
                while sent < total {
                    let size = FLOOD_CHUNK_BYTES.min(total - sent);
                    if stream.write_all(&chunk[..size]).await.is_err() {
                        return;
                    }
                    sent += size;
                    flooded.fetch_add(size, Ordering::SeqCst);
                }
                return;
            }
        }
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<SeenRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let target = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Some(SeenRequest {
        method,
        target,
        headers,
        body,
    })
}

pub fn loopback_config() -> GuardConfig {
    GuardConfig {
        allow: vec!["127.0.0.1/32".parse().unwrap()],
        connect_timeout: Duration::from_secs(2),
        hop_timeout: Duration::from_secs(5),
        total_timeout: Duration::from_secs(10),
        ..GuardConfig::default()
    }
}

pub fn client_with(config: GuardConfig, lookup: Arc<StaticLookup>) -> GuardedClient {
    GuardedClient::with_lookup(config, lookup).unwrap()
}

pub fn client(config: GuardConfig) -> GuardedClient {
    client_with(config, Arc::new(StaticLookup::new()))
}

pub fn get(url: &str, config: &GuardConfig) -> OutboundRequest {
    OutboundRequest::new(
        http::Method::GET,
        validate_target_url(url, config).unwrap(),
        http::HeaderMap::new(),
        bytes::Bytes::new(),
    )
}
