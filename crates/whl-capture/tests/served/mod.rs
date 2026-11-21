#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use axum::http::HeaderMap;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use whl_core::endpoint_id::EndpointId;

use crate::common::Harness;

pub async fn harness_with_slots(max_delayed: usize) -> Harness {
    let db = whl_testkit::TestDb::new().await;
    let state = whl_capture::CaptureState::new(
        whl_store::Store::from_pool(db.pool().clone()),
        whl_capture::CaptureSettings {
            limits: crate::common::default_limits(),
            trusted_proxies: Vec::new(),
            public_url: crate::common::PUBLIC_URL.to_owned(),
            max_delayed,
        },
    )
    .expect("settings are valid");
    let router = whl_capture::router(state.clone());
    Harness { db, state, router }
}

pub async fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..1500 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[derive(Debug, Clone)]
pub struct Configured {
    pub status: i16,
    pub content_type: &'static str,
    pub headers: Vec<(&'static str, &'static str)>,
    pub body: Vec<u8>,
    pub delay_ms: i32,
    pub cors: bool,
}

impl Default for Configured {
    fn default() -> Self {
        Self {
            status: 200,
            content_type: "text/plain",
            headers: Vec::new(),
            body: Vec::new(),
            delay_ms: 0,
            cors: false,
        }
    }
}

pub async fn endpoint_configured(harness: &Harness, configured: Configured) -> EndpointId {
    let id = harness.endpoint().await;
    let headers: Vec<_> = configured
        .headers
        .iter()
        .map(|(name, value)| json!({"name": name, "value": value}))
        .collect();
    sqlx::query(
        "UPDATE endpoints SET response_status = $2, response_content_type = $3, \
         response_headers = $4::text::jsonb, response_body = $5, response_delay_ms = $6, cors = $7 \
         WHERE id = $1",
    )
    .bind(id.unpack())
    .bind(configured.status)
    .bind(configured.content_type)
    .bind(serde_json::to_string(&headers).expect("headers encode"))
    .bind(&configured.body)
    .bind(configured.delay_ms)
    .bind(configured.cors)
    .execute(harness.db.pool())
    .await
    .expect("endpoint configures");
    id
}

pub fn sorted_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    pairs.sort();
    pairs
}

pub fn expected(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    list.sort();
    list
}

pub async fn serve(harness: &Harness) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let addr = listener.local_addr().expect("local address");
    let app = whl_capture::router(harness.state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    addr
}

pub struct Wire {
    stream: TcpStream,
    pending: Vec<u8>,
}

#[derive(Debug)]
pub struct Head {
    pub status: u16,
    pub text: String,
    pub raw: Vec<u8>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<String> {
        self.text.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .text
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(key, _)| key.to_ascii_lowercase())
            .collect();
        names.sort();
        names
    }
}

impl Wire {
    pub async fn connect(addr: SocketAddr) -> Self {
        Self {
            stream: TcpStream::connect(addr).await.expect("connects"),
            pending: Vec::new(),
        }
    }

    pub async fn write(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.expect("socket writes");
    }

    pub async fn head(&mut self) -> Head {
        let deadline = Duration::from_secs(5);
        let head_end = loop {
            if let Some(position) = self
                .pending
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
                break position;
            }
            let mut buffer = [0_u8; 4096];
            let read = tokio::time::timeout(deadline, self.stream.read(&mut buffer))
                .await
                .expect("the server answers in time")
                .expect("socket reads");
            assert!(read > 0, "the connection closed before a full head");
            self.pending.extend_from_slice(&buffer[..read]);
        };
        let raw: Vec<u8> = self.pending.drain(..head_end + 4).collect();
        let text = String::from_utf8_lossy(&raw[..head_end]).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status line");
        Head { status, text, raw }
    }

    pub async fn body(&mut self, length: usize) -> Vec<u8> {
        while self.pending.len() < length {
            let mut buffer = [0_u8; 4096];
            let read = tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buffer))
                .await
                .expect("the body arrives in time")
                .expect("socket reads");
            assert!(read > 0, "the connection closed inside the body");
            self.pending.extend_from_slice(&buffer[..read]);
        }
        self.pending.drain(..length).collect()
    }

    pub async fn leftover_after(&mut self, quiet: Duration) -> Vec<u8> {
        let mut buffer = [0_u8; 4096];
        if let Ok(Ok(read)) = tokio::time::timeout(quiet, self.stream.read(&mut buffer)).await {
            self.pending.extend_from_slice(&buffer[..read]);
        }
        std::mem::take(&mut self.pending)
    }

    pub async fn closed_by_server(&mut self) -> bool {
        let mut buffer = [0_u8; 1024];
        match tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buffer)).await {
            Ok(Ok(0)) => true,
            Ok(Err(error)) => error.kind() == std::io::ErrorKind::ConnectionReset,
            _ => false,
        }
    }
}
