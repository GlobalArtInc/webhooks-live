#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use sqlx::Row;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use whl_core::endpoint_id::EndpointId;
use whl_mail::{MailDomain, MailLimits, MailServer, MailSettings};
use whl_store::Store;
use whl_store::captures::CapturePolicy;
use whl_testkit::TestDb;

pub const DOMAIN: &str = "mail.test";
pub const WAIT: Duration = Duration::from_secs(10);

pub fn settings() -> MailSettings {
    MailSettings {
        listen_addr: "127.0.0.1:0".parse().expect("address"),
        domain: MailDomain::parse(DOMAIN).expect("domain"),
        trusted_proxies: Vec::new(),
        tls: None,
        limits: MailLimits::default(),
        capture: CapturePolicy {
            max_captures: 500,
            retention: Duration::from_secs(7 * 24 * 3600),
        },
        storage_full: None,
    }
}

pub struct Mail {
    pub db: TestDb,
    pub store: Store,
    pub addr: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Mail {
    pub async fn start() -> Self {
        Self::start_with(settings()).await
    }

    pub async fn start_with(settings: MailSettings) -> Self {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let server = MailServer::bind(settings, store.clone())
            .await
            .expect("the mail server binds");
        let addr = server.local_addr();
        let (stop, stop_rx) = watch::channel(false);
        let task = tokio::spawn(server.serve(stop_rx));
        Self {
            db,
            store,
            addr,
            stop,
            task,
        }
    }

    pub async fn shutdown(self) {
        self.stop.send(true).expect("the server is running");
        tokio::time::timeout(WAIT, self.task)
            .await
            .expect("the server stops")
            .expect("the server task ends cleanly");
    }

    pub async fn endpoint(&self, email_enabled: bool) -> EndpointId {
        let id = EndpointId::generate().expect("entropy");
        sqlx::query("INSERT INTO endpoints (id, email_enabled) VALUES ($1, $2)")
            .bind(id.unpack())
            .bind(email_enabled)
            .execute(self.db.pool())
            .await
            .expect("endpoint insert");
        id
    }

    pub async fn allow_only(&self, id: &EndpointId, cidr: &str) {
        sqlx::query("UPDATE endpoints SET sender_allow_cidrs = ARRAY[$2]::cidr[] WHERE id = $1")
            .bind(id.unpack())
            .bind(cidr)
            .execute(self.db.pool())
            .await
            .expect("allow list update");
    }

    pub async fn captures(&self, id: &EndpointId) -> Vec<StoredMail> {
        let rows = sqlx::query(
            "SELECT c.id::text AS id, c.kind, c.method, c.url, c.path, c.host, \
               host(c.client_ip) AS client_ip, c.response_status, c.content_type, \
               c.meta::text AS meta, c.headers::text AS headers, b.data \
             FROM captures c JOIN capture_bodies b ON b.capture_id = c.id \
             WHERE c.endpoint_id = $1 ORDER BY c.seq",
        )
        .bind(id.unpack())
        .fetch_all(self.db.pool())
        .await
        .expect("capture query");
        rows.iter()
            .map(|row| StoredMail {
                id: row.get("id"),
                kind: row.get("kind"),
                method: row.get("method"),
                url: row.get("url"),
                path: row.get("path"),
                host: row.get("host"),
                client_ip: row.get("client_ip"),
                response_status: row.get("response_status"),
                content_type: row.get("content_type"),
                meta: serde_json::from_str(&row.get::<String, _>("meta")).expect("meta JSON"),
                headers: serde_json::from_str(&row.get::<String, _>("headers"))
                    .expect("headers JSON"),
                body: row.get("data"),
            })
            .collect()
    }

    pub async fn capture_count(&self, id: &EndpointId) -> usize {
        self.captures(id).await.len()
    }
}

#[derive(Debug)]
pub struct StoredMail {
    pub id: String,
    pub kind: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub host: String,
    pub client_ip: String,
    pub response_status: i16,
    pub content_type: Option<String>,
    pub meta: serde_json::Value,
    pub headers: serde_json::Value,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub code: u16,
    pub text: String,
}

impl Reply {
    pub fn last_line(&self) -> &str {
        self.text.lines().last().unwrap_or_default()
    }
}

pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}

pub struct Client {
    stream: BufReader<Box<dyn Duplex>>,
}

impl Client {
    pub async fn connect(addr: SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.expect("connect");
        Self::from_stream(Box::new(stream))
    }

    pub fn from_stream(stream: Box<dyn Duplex>) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    pub fn into_stream(self) -> Box<dyn Duplex> {
        self.stream.into_inner()
    }

    pub async fn greeted(addr: SocketAddr) -> Self {
        let mut client = Self::connect(addr).await;
        assert_eq!(client.read_reply().await.code, 220);
        let ehlo = client.say("EHLO client.example.org").await;
        assert_eq!(ehlo.code, 250, "{ehlo:?}");
        client
    }

    pub async fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.get_mut().write_all(bytes).await.expect("write");
    }

    pub async fn try_read_reply(&mut self) -> Option<Reply> {
        let mut text = String::new();
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(WAIT, self.stream.read_line(&mut line))
                .await
                .expect("the server answers in time");
            match read {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            text.push_str(&line);
            let bytes = line.as_bytes();
            if bytes.len() >= 4 && bytes[3] == b' ' || bytes.len() == 3 {
                let code = line[..3].parse().expect("a numeric reply code");
                return Some(Reply {
                    code,
                    text: text.trim_end().to_owned(),
                });
            }
        }
    }

    pub async fn read_reply(&mut self) -> Reply {
        self.try_read_reply().await.expect("a reply before EOF")
    }

    pub async fn say(&mut self, line: &str) -> Reply {
        self.send_raw(format!("{line}\r\n").as_bytes()).await;
        self.read_reply().await
    }

    pub async fn expect_closed(&mut self) {
        let mut rest = Vec::new();
        let outcome = tokio::time::timeout(WAIT, self.stream.read_to_end(&mut rest))
            .await
            .expect("the server closes in time");
        assert!(
            outcome.is_err() || rest.is_empty(),
            "unexpected bytes before close: {:?}",
            String::from_utf8_lossy(&rest)
        );
    }

    pub async fn envelope(&mut self, recipient: &str) -> Reply {
        let sender = self.say("MAIL FROM:<sender@example.org>").await;
        assert_eq!(sender.code, 250, "{sender:?}");
        self.say(&format!("RCPT TO:<{recipient}>")).await
    }

    pub async fn data(&mut self, content: &str) -> Reply {
        let start = self.say("DATA").await;
        assert_eq!(start.code, 354, "{start:?}");
        self.send_raw(content.as_bytes()).await;
        self.send_raw(b"\r\n.\r\n").await;
        self.read_reply().await
    }

    pub async fn deliver(&mut self, recipient: &str, content: &str) -> Reply {
        let accepted = self.envelope(recipient).await;
        assert_eq!(accepted.code, 250, "{accepted:?}");
        self.data(content).await
    }
}

pub fn address(id: &EndpointId) -> String {
    format!("{}@{DOMAIN}", id.unpack())
}

pub const SIMPLE: &str = "From: Alice <alice@example.org>\r\nTo: bob@example.org\r\nSubject: Hello there\r\nDate: Tue, 17 Nov 2026 10:00:00 +0000\r\nMessage-ID: <abc123@example.org>\r\n\r\nHello body";
