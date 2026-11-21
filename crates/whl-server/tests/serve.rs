use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use whl_core::config::Config;
use whl_server::ServerError;
use whl_store::{Store, StoreError};
use whl_testkit::TestDb;

struct Reply {
    status: u16,
    content_type: String,
    body: String,
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
    let content_type = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-type")
                .then(|| value.trim().to_owned())
        })
        .unwrap_or_default();

    Ok(Reply {
        status,
        content_type,
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
    assert_eq!((unknown.status, unknown.body.as_str()), (404, "not found"));

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
