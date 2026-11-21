use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use whl_core::config::Config;
use whl_server::{HttpLimits, ServerError};
use whl_store::{Store, StoreError};
use whl_testkit::TestDb;

struct Reply {
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

async fn get(addr: SocketAddr, path: &str) -> io::Result<Reply> {
    let mut stream = TcpStream::connect(addr).await?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;

    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::other("response has no header block"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::other("response has no status code"))?;
    let headers: Vec<(String, String)> = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();
    let content_type = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.clone())
        .unwrap_or_default();

    Ok(Reply {
        status,
        content_type,
        headers,
        body: body.to_owned(),
    })
}

#[allow(clippy::result_large_err)]
fn load_config(database_url: &str) -> Config {
    let mut result = None;
    Jail::expect_with(|jail| {
        let vars = [
            ("WEBHOOKS_LIVE_PUBLIC_URL", "http://127.0.0.1"),
            ("WEBHOOKS_LIVE_DATABASE_URL", database_url),
            ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_ADMIN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "1s"),
            ("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "5s"),
        ];
        for (key, value) in vars {
            jail.set_env(key, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result
        .expect("the jail closure always runs")
        .expect("test configuration loads")
}

async fn wait_ready(admin: SocketAddr) {
    for _ in 0..500 {
        let ready = get(admin, "/readyz")
            .await
            .is_ok_and(|reply| reply.status == 200);
        if ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("readyz never answered 200");
}

#[tokio::test]
async fn serves_probes_and_drains_when_shutdown_fires() {
    let db = TestDb::new().await;
    let config = load_config(db.url());
    let store = Store::connect(&config.database)
        .await
        .expect("the test database accepts connections");
    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");
    let public = bound.public_addr();
    let admin = bound.admin_addr();

    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));

    let healthz = get(admin, "/healthz").await.expect("admin answers");
    assert_eq!((healthz.status, healthz.body.as_str()), (200, "ok"));

    wait_ready(admin).await;
    let readyz = get(admin, "/readyz").await.expect("admin answers");
    assert_eq!((readyz.status, readyz.body.as_str()), (200, "ok"));

    let metrics = get(admin, "/metrics").await.expect("admin answers");
    assert_eq!(metrics.status, 200);
    assert!(
        metrics
            .content_type
            .starts_with("text/plain; version=0.0.4")
    );
    assert!(metrics.body.lines().all(|line| line.starts_with('#')
        || line.split_whitespace().count() == 2
        || line.is_empty()));

    let unknown = get(public, "/no/such/path").await.expect("public answers");
    assert_eq!((unknown.status, unknown.body.as_str()), (404, ""));

    trigger
        .send(())
        .expect("server waits for the shutdown future");

    let mut draining = false;
    for _ in 0..100 {
        match get(admin, "/readyz").await {
            Ok(reply) if reply.status == 503 => {
                draining = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    assert!(draining, "readyz turned 503 during the drain");

    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("run returns within the grace period")
        .expect("server task joins")
        .expect("serve returns Ok after shutdown");
}

#[tokio::test]
async fn run_returns_ok_when_the_shutdown_future_completes() {
    let db = TestDb::new().await;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        whl_server::run(load_config(db.url()), async {}),
    )
    .await
    .expect("run returns within the grace period");

    result.expect("run exits cleanly");
}

#[tokio::test]
async fn never_reports_ready_while_a_migration_is_missing() {
    let db = TestDb::new().await;
    Store::from_pool(db.pool().clone())
        .migrate_down()
        .await
        .expect("the last migration takes back");
    let config = load_config(db.url());
    let store = Store::connect(&config.database)
        .await
        .expect("the test database accepts connections");
    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");
    let admin = bound.admin_addr();

    let server = tokio::spawn(bound.serve(store, std::future::pending::<()>()));
    let probing = async {
        while !server.is_finished() {
            if let Ok(reply) = get(admin, "/readyz").await {
                assert_ne!(
                    reply.status, 200,
                    "readyz answered 200 while a migration is missing"
                );
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), probing)
        .await
        .expect("serve returns once the schema check fails");

    let outcome = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("serve returns once the schema check fails")
        .expect("server task joins");
    assert!(
        matches!(
            outcome,
            Err(ServerError::Database(StoreError::SchemaBehind { .. }))
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn sends_bye_to_open_streams_after_the_drain_and_closes_them() {
    let db = TestDb::new().await;
    let endpoint = "0123456789abcdefghjkmnpq";
    sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
        .bind(endpoint)
        .execute(db.pool())
        .await
        .expect("the endpoint inserts");
    let config = load_config(db.url());
    let store = Store::connect(&config.database)
        .await
        .expect("the test database accepts connections");
    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");
    let public = bound.public_addr();
    let admin = bound.admin_addr();
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_ready(admin).await;

    let mut stream = TcpStream::connect(public).await.expect("public accepts");
    let request = format!(
        "GET /api/v1/endpoints/{endpoint}/stream HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("the request is written");
    let mut received = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !String::from_utf8_lossy(&received).contains("event: ready") {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .expect("the stream starts")
            .expect("the socket reads");
        assert!(read > 0, "the stream closed before ready");
        received.extend_from_slice(&chunk[..read]);
    }

    let shutdown_at = std::time::Instant::now();
    trigger
        .send(())
        .expect("server waits for the shutdown future");
    tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut received))
        .await
        .expect("the stream closes")
        .expect("the socket reads to the end");

    let text = String::from_utf8_lossy(&received);
    assert!(text.contains("event: bye"), "{text}");
    assert!(
        shutdown_at.elapsed() >= Duration::from_millis(900),
        "bye must follow the drain wait"
    );
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("run returns within the grace period")
        .expect("server task joins")
        .expect("serve returns Ok after shutdown");
}

#[tokio::test]
async fn mounts_the_management_api_on_the_public_listener() {
    let db = TestDb::new().await;
    let config = load_config(db.url());
    let store = Store::connect(&config.database)
        .await
        .expect("the test database accepts connections");
    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");
    let public = bound.public_addr();

    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));

    let instance = get(public, "/api/v1/instance")
        .await
        .expect("public answers");
    assert_eq!(instance.status, 200);
    assert!(instance.body.contains("\"auth_mode\":\"open\""));

    let missing = get(public, "/api/v1/nothing")
        .await
        .expect("public answers");
    assert_eq!(
        (missing.status, missing.content_type.as_str()),
        (404, "application/json")
    );

    trigger
        .send(())
        .expect("server waits for the shutdown future");
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("run returns within the grace period")
        .expect("server task joins")
        .expect("serve returns Ok after shutdown");
}

struct Running {
    public: SocketAddr,
    trigger: oneshot::Sender<()>,
    server: JoinHandle<Result<(), ServerError>>,
}

impl Running {
    async fn start(db: &TestDb, limits: HttpLimits) -> Self {
        let config = load_config(db.url());
        let store = Store::connect(&config.database)
            .await
            .expect("the test database accepts connections");
        let bound = whl_server::bind(&config)
            .await
            .expect("ephemeral ports bind")
            .with_http_limits(limits);
        let public = bound.public_addr();
        let admin = bound.admin_addr();
        let (trigger, stop) = oneshot::channel::<()>();
        let server = tokio::spawn(bound.serve(store, async {
            let _ = stop.await;
        }));
        wait_ready(admin).await;
        Self {
            public,
            trigger,
            server,
        }
    }

    async fn stop(self) {
        self.trigger
            .send(())
            .expect("server waits for the shutdown future");
        tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .expect("serve returns within the grace period")
            .expect("server task joins")
            .expect("serve returns Ok after shutdown");
    }
}

async fn insert_endpoint(db: &TestDb) -> whl_core::endpoint_id::EndpointId {
    let id = whl_core::endpoint_id::EndpointId::generate().expect("entropy is available");
    sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
        .bind(id.unpack())
        .execute(db.pool())
        .await
        .expect("endpoint inserts");
    id
}

#[tokio::test]
async fn public_listener_captures_on_an_endpoint_id() {
    let db = TestDb::new().await;
    let running = Running::start(&db, HttpLimits::default()).await;
    let id = insert_endpoint(&db).await;

    let stored = get(running.public, &format!("/{id}/hook?a=1"))
        .await
        .expect("public answers");
    assert_eq!(stored.status, 200);
    let gone = get(running.public, "/zzzzzzzzzzzzzzzzzzzzzzzz")
        .await
        .expect("public answers");
    assert_eq!(gone.status, 410);

    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT method, host(client_ip) FROM captures WHERE endpoint_id = $1")
            .bind(id.unpack())
            .fetch_all(db.pool())
            .await
            .expect("captures read");
    assert_eq!(rows, [("GET".to_owned(), "127.0.0.1".to_owned())]);
    running.stop().await;
}

#[tokio::test]
async fn public_listener_serves_the_ui_for_everything_that_is_not_an_endpoint() {
    let db = TestDb::new().await;
    let running = Running::start(&db, HttpLimits::default()).await;
    let id = insert_endpoint(&db).await;
    let valid = id.unpack().to_owned();

    let root = get(running.public, "/").await.expect("public answers");
    assert_eq!(root.status, 200);
    assert!(
        root.body.contains("The front end is not built"),
        "{}",
        root.body
    );
    assert!(root.content_type.starts_with("text/html"));
    assert_eq!(root.header("cache-control"), Some("no-cache"));
    assert_eq!(root.header("x-frame-options"), Some("DENY"));
    assert_eq!(root.header("referrer-policy"), Some("no-referrer"));
    assert!(
        root.header("content-security-policy")
            .is_some_and(|policy| policy.contains("default-src 'none'"))
    );

    let ui_paths = [
        format!("/e/{valid}"),
        "/settings".to_owned(),
        "/endpoints".to_owned(),
    ];
    for path in &ui_paths {
        let reply = get(running.public, path).await.expect("public answers");
        assert_eq!(reply.status, 200, "{path}");
        assert!(reply.body.contains("The front end is not built"), "{path}");
        assert_eq!(reply.header("x-webhooks-live-error"), None, "{path}");
    }

    let missing = get(running.public, "/assets/missing.js")
        .await
        .expect("public answers");
    assert_eq!((missing.status, missing.body.as_str()), (404, ""));
    assert_eq!(missing.header("x-webhooks-live-error"), None);

    let instance = get(running.public, "/api/v1/instance")
        .await
        .expect("public answers");
    assert_eq!(instance.status, 200);
    assert!(instance.body.contains("\"auth_mode\":\"open\""));

    let invalid = [
        valid.to_uppercase(),
        valid[..23].to_owned(),
        format!("{valid}a"),
        format!("{}i", &valid[..23]),
    ];
    for segment in &invalid {
        let reply = get(running.public, &format!("/{segment}"))
            .await
            .expect("public answers");
        assert_eq!(reply.status, 404, "{segment}");
        assert_eq!(reply.header("x-webhooks-live-error"), None, "{segment}");
        assert_eq!(reply.body, "", "{segment}");
    }

    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM captures")
        .fetch_one(db.pool())
        .await
        .expect("count reads");
    assert_eq!(total, 0);
    running.stop().await;
}

const SHORT_HEADER_TIMEOUT: Duration = Duration::from_millis(400);

fn short_timeout_limits() -> HttpLimits {
    HttpLimits {
        header_read_timeout: SHORT_HEADER_TIMEOUT,
        ..HttpLimits::default()
    }
}

async fn raw_exchange(addr: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut stream = TcpStream::connect(addr).await.expect("public accepts");
    let _ = stream.write_all(payload).await;
    let mut received = Vec::new();
    let mut chunk = [0_u8; 4096];
    let reading = async {
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => received.extend_from_slice(&chunk[..read]),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), reading)
        .await
        .expect("the server closes the connection");
    received
}

fn request_with_headers(count: usize, value_len: usize) -> Vec<u8> {
    let mut request =
        String::from("GET /api/v1/instance HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
    for index in 0..count {
        request.push_str(&format!("X-Pad-{index}: {}\r\n", "a".repeat(value_len)));
    }
    request.push_str("\r\n");
    request.into_bytes()
}

fn status_line(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn refuses_more_than_the_header_count_limit_and_accepts_exactly_the_limit() {
    let db = TestDb::new().await;
    let running = Running::start(&db, HttpLimits::default()).await;

    let at_limit = raw_exchange(running.public, &request_with_headers(98, 1)).await;
    assert_eq!(status_line(&at_limit), "HTTP/1.1 200 OK");

    let over = raw_exchange(running.public, &request_with_headers(99, 1)).await;
    assert_eq!(
        status_line(&over),
        "HTTP/1.1 431 Request Header Fields Too Large",
        "{}",
        String::from_utf8_lossy(&over)
    );
    running.stop().await;
}

#[tokio::test]
async fn refuses_a_header_block_over_the_size_limit() {
    let db = TestDb::new().await;
    let running = Running::start(&db, HttpLimits::default()).await;

    let under = raw_exchange(running.public, &request_with_headers(6, 8 * 1024)).await;
    assert_eq!(status_line(&under), "HTTP/1.1 200 OK");

    let over = raw_exchange(running.public, &request_with_headers(1, 70 * 1024)).await;
    assert_eq!(
        status_line(&over),
        "HTTP/1.1 431 Request Header Fields Too Large",
        "{}",
        String::from_utf8_lossy(&over)
    );
    running.stop().await;
}

async fn closes_after_stalling(addr: SocketAddr, sent: &[u8], pause: Duration) -> Duration {
    let mut stream = TcpStream::connect(addr).await.expect("public accepts");
    let started = std::time::Instant::now();
    for byte in sent {
        stream
            .write_all(std::slice::from_ref(byte))
            .await
            .expect("the byte is written");
        tokio::time::sleep(pause).await;
    }
    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest))
        .await
        .expect("the server disconnects the stalled client")
        .ok();
    started.elapsed()
}

#[tokio::test]
async fn disconnects_a_client_that_trickles_headers_and_stalls() {
    let db = TestDb::new().await;
    let running = Running::start(&db, short_timeout_limits()).await;

    let elapsed = closes_after_stalling(
        running.public,
        b"GET /api/v1/instance HTTP/1.1\r\nHost: loc",
        Duration::from_millis(5),
    )
    .await;

    assert!(elapsed >= SHORT_HEADER_TIMEOUT, "{elapsed:?}");
    assert!(elapsed < SHORT_HEADER_TIMEOUT * 5, "{elapsed:?}");
    running.stop().await;
}

#[tokio::test]
async fn disconnects_a_client_that_stalls_inside_a_possible_http2_preface() {
    let db = TestDb::new().await;
    let running = Running::start(&db, short_timeout_limits()).await;

    let elapsed = closes_after_stalling(running.public, b"P", Duration::from_millis(5)).await;

    assert!(elapsed >= SHORT_HEADER_TIMEOUT, "{elapsed:?}");
    assert!(elapsed < SHORT_HEADER_TIMEOUT * 5, "{elapsed:?}");
    running.stop().await;
}

#[tokio::test]
async fn keeps_serving_requests_on_one_connection_and_large_bodies_after_the_limits() {
    let db = TestDb::new().await;
    let running = Running::start(&db, short_timeout_limits()).await;
    let id = insert_endpoint(&db).await;

    let mut stream = TcpStream::connect(running.public)
        .await
        .expect("public accepts");
    for _ in 0..2 {
        stream
            .write_all(b"GET /api/v1/instance HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .expect("the request is written");
        let mut chunk = [0_u8; 4096];
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut chunk))
            .await
            .expect("the server answers")
            .expect("the socket reads");
        assert!(chunk[..read].starts_with(b"HTTP/1.1 200 OK"));
    }

    let body = "x".repeat(300 * 1024);
    let request = format!(
        "POST /{id}/big HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let reply = raw_exchange(running.public, request.as_bytes()).await;
    assert_eq!(status_line(&reply), "HTTP/1.1 200 OK");
    let stored: i32 = sqlx::query_scalar("SELECT body_size FROM captures WHERE endpoint_id = $1")
        .bind(id.unpack())
        .fetch_one(db.pool())
        .await
        .expect("the capture reads");
    assert_eq!(stored, 300 * 1024);
    running.stop().await;
}
