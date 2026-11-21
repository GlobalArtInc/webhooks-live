mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::{Harness, default_limits};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use whl_capture::CaptureLimits;

struct Wire {
    stream: TcpStream,
    pending: Vec<u8>,
}

struct Head {
    status: u16,
    text: String,
}

impl Head {
    fn header(&self, name: &str) -> Option<String> {
        self.text.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }
}

impl Wire {
    async fn connect(addr: SocketAddr) -> Self {
        Self {
            stream: TcpStream::connect(addr).await.expect("connects"),
            pending: Vec::new(),
        }
    }

    async fn write(&mut self, bytes: &[u8]) {
        let _ = self.stream.write_all(bytes).await;
    }

    async fn response(&mut self) -> (Head, String) {
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
            assert!(read > 0, "the connection closed before a full response");
            self.pending.extend_from_slice(&buffer[..read]);
        };
        let text = String::from_utf8_lossy(&self.pending[..head_end]).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status line");
        let head = Head { status, text };
        let length: usize = head
            .header("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let body_start = head_end + 4;
        while self.pending.len() < body_start + length {
            let mut buffer = [0_u8; 4096];
            let read = tokio::time::timeout(deadline, self.stream.read(&mut buffer))
                .await
                .expect("the body arrives in time")
                .expect("socket reads");
            assert!(read > 0, "the connection closed inside the body");
            self.pending.extend_from_slice(&buffer[..read]);
        }
        let body =
            String::from_utf8_lossy(&self.pending[body_start..body_start + length]).into_owned();
        self.pending.drain(..body_start + length);
        (head, body)
    }

    async fn closed_by_server(&mut self) -> bool {
        let mut buffer = [0_u8; 1024];
        match tokio::time::timeout(Duration::from_secs(5), self.stream.read(&mut buffer)).await {
            Ok(Ok(0)) => true,
            Ok(Err(error)) => error.kind() == std::io::ErrorKind::ConnectionReset,
            _ => false,
        }
    }
}

async fn serve(harness: &Harness) -> SocketAddr {
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

#[tokio::test]
async fn early_413_by_content_length_closes_the_connection() {
    let harness = Harness::with_limits(CaptureLimits {
        max_body_bytes: 1000,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    let addr = serve(&harness).await;

    let mut wire = Wire::connect(addr).await;
    wire.write(
        format!(
            "POST /{id} HTTP/1.1\r\nHost: t\r\nContent-Length: 5000\r\nContent-Type: text/plain\r\n\r\n"
        )
        .as_bytes(),
    )
    .await;
    let (head, body) = wire.response().await;
    assert_eq!(head.status, 413);
    assert_eq!(head.header("connection").as_deref(), Some("close"));
    assert_eq!(body, "Request body exceeds 1000 bytes.");
    assert!(wire.closed_by_server().await);
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn chunked_overflow_answers_413_and_closes_the_connection() {
    let harness = Harness::with_limits(CaptureLimits {
        max_body_bytes: 1000,
        ..default_limits()
    })
    .await;
    let id = harness.endpoint().await;
    let addr = serve(&harness).await;

    let mut wire = Wire::connect(addr).await;
    wire.write(
        format!("POST /{id} HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n").as_bytes(),
    )
    .await;
    for _ in 0..2 {
        wire.write(format!("258\r\n{}\r\n", "x".repeat(600)).as_bytes())
            .await;
    }
    let (head, _) = wire.response().await;
    assert_eq!(head.status, 413);
    assert_eq!(head.header("connection").as_deref(), Some("close"));
    assert!(wire.closed_by_server().await);
    assert!(harness.rows(&id).await.is_empty());
}

#[tokio::test]
async fn bodiless_refusal_and_stored_capture_keep_the_connection_alive() {
    let harness = Harness::new().await;
    let id = harness.endpoint().await;
    let addr = serve(&harness).await;

    let mut wire = Wire::connect(addr).await;
    wire.write(b"GET /not-an-id HTTP/1.1\r\nHost: t\r\n\r\n")
        .await;
    let (head, body) = wire.response().await;
    assert_eq!((head.status, body.as_str()), (404, "Not found."));
    assert!(head.header("connection").is_none());

    wire.write(
        format!("POST /{id} HTTP/1.1\r\nHost: t\r\nContent-Length: 4\r\n\r\nabcd").as_bytes(),
    )
    .await;
    let (head, _) = wire.response().await;
    assert_eq!(head.status, 200);
    assert!(head.header("connection").is_none());
    assert_eq!(head.header("x-endpoint-id").as_deref(), Some(id.unpack()));

    wire.write(format!("GET /{id}/second HTTP/1.1\r\nHost: t\r\n\r\n").as_bytes())
        .await;
    let (head, _) = wire.response().await;
    assert_eq!(head.status, 200);

    let rows = harness.rows(&id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].body, b"abcd");
    assert_eq!(rows[0].client_ip, "127.0.0.1");
    assert_eq!(rows[0].host, "t");
    assert_eq!(rows[0].url, format!("http://t/{id}"));
}
