use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, PgPool};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use whl_core::config::Config;
use whl_core::endpoint_id::EndpointId;
use whl_server::{BackgroundSettings, ServerError};
use whl_store::Store;
use whl_store::maintenance::LEADER_LOCK_KEY;
use whl_testkit::TestDb;

const OUTER_DEADLINE: Duration = Duration::from_secs(30);

struct Reply {
    status: u16,
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

fn parse_head(head: &str) -> io::Result<(u16, Vec<(String, String)>)> {
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::other("response has no status code"))?;
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_owned(), value.trim().to_owned()))
        })
        .collect();
    Ok((status, headers))
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
    let (status, headers) = parse_head(head)?;
    Ok(Reply {
        status,
        headers,
        body: body.to_owned(),
    })
}

async fn stream_head(addr: SocketAddr, endpoint: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).await.expect("public accepts");
    let request = format!(
        "GET /api/v1/endpoints/{endpoint}/stream HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("the request is written");
    let mut received = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !String::from_utf8_lossy(&received).contains("\r\n\r\n") {
        let read = tokio::time::timeout(OUTER_DEADLINE, stream.read(&mut chunk))
            .await
            .expect("the stream answers")
            .expect("the socket reads");
        assert!(read > 0, "the stream closed before its head");
        received.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&received).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("the head is complete");
    let (status, headers) = parse_head(head).expect("the head parses");
    Reply {
        status,
        headers,
        body: body.to_owned(),
    }
}

#[allow(clippy::result_large_err)]
fn load_config(database_url: &str, extra: &[(&str, &str)]) -> Config {
    let mut result = None;
    Jail::expect_with(|jail| {
        let vars = [
            ("WEBHOOKS_LIVE_PUBLIC_URL", "http://127.0.0.1"),
            ("WEBHOOKS_LIVE_DATABASE_URL", database_url),
            ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_ADMIN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "0s"),
            ("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "60s"),
        ];
        for (key, value) in vars.iter().chain(extra.iter()) {
            jail.set_env(key, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result
        .expect("the jail closure always runs")
        .expect("test configuration loads")
}

fn quick_background(config: &Config) -> BackgroundSettings {
    let mut settings = BackgroundSettings::from_config(config);
    settings.storage_refresh = Duration::from_millis(50);
    settings.maintenance.cycle_interval = Duration::from_millis(100);
    settings.maintenance.retry_interval = Duration::from_millis(100);
    settings.maintenance.lock_check_interval = Duration::from_millis(50);
    settings
}

struct Running {
    public: SocketAddr,
    trigger: oneshot::Sender<()>,
    server: JoinHandle<Result<(), ServerError>>,
}

impl Running {
    async fn start(db: &TestDb, extra: &[(&str, &str)]) -> Self {
        let config = load_config(db.url(), extra);
        let store = Store::connect(&config.database)
            .await
            .expect("the test database accepts connections");
        let bound = whl_server::bind(&config)
            .await
            .expect("ephemeral ports bind")
            .with_background(quick_background(&config));
        let public = bound.public_addr();
        let admin = bound.admin_addr();
        let (trigger, stop) = oneshot::channel::<()>();
        let server = tokio::spawn(bound.serve(store, async {
            let _ = stop.await;
        }));
        for _ in 0..500 {
            if get(admin, "/readyz")
                .await
                .is_ok_and(|reply| reply.status == 200)
            {
                return Self {
                    public,
                    trigger,
                    server,
                };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("readyz never answered 200");
    }

    async fn stop(self) {
        self.trigger
            .send(())
            .expect("server waits for the shutdown future");
        tokio::time::timeout(OUTER_DEADLINE, self.server)
            .await
            .expect("serve returns within the grace period")
            .expect("server task joins")
            .expect("serve returns Ok after shutdown");
    }
}

async fn insert_endpoint(pool: &PgPool, bytes: i64, active_ago_secs: i64) -> String {
    let id = EndpointId::generate()
        .expect("entropy is available")
        .unpack()
        .to_owned();
    sqlx::query(
        "INSERT INTO endpoints (id, bytes_used, last_active_at) \
         VALUES ($1, $2, now() - make_interval(secs => $3))",
    )
    .bind(&id)
    .bind(bytes)
    .bind(active_ago_secs as f64)
    .execute(pool)
    .await
    .expect("the endpoint inserts");
    id
}

async fn advisory_holders(pool: &PgPool) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT pid FROM pg_locks WHERE locktype = 'advisory' AND granted \
         AND database = (SELECT oid FROM pg_database WHERE datname = current_database())",
    )
    .fetch_all(pool)
    .await
    .expect("locks read")
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

const RATE_PERIOD_SECS: u64 = 60;

fn assert_retry_after_within_period(reply: &Reply) {
    let advertised: u64 = reply
        .header("retry-after")
        .expect("the refusal carries retry-after")
        .parse()
        .expect("retry-after is whole seconds");
    assert!(
        (1..=RATE_PERIOD_SECS).contains(&advertised),
        "retry-after {advertised} is outside 1..={RATE_PERIOD_SECS}"
    );
}

#[tokio::test]
async fn rate_limits_are_wired_into_capture_api_and_stream() {
    let db = TestDb::new().await;
    let running = Running::start(
        &db,
        &[
            ("WEBHOOKS_LIVE_RATE_CAPTURE_IP", "1/m:2"),
            ("WEBHOOKS_LIVE_RATE_API_IP", "1/m:4"),
        ],
    )
    .await;
    let id = insert_endpoint(db.pool(), 0, 0).await;

    for _ in 0..2 {
        let stored = get(running.public, &format!("/{id}"))
            .await
            .expect("public answers");
        assert_eq!(stored.status, 200);
    }
    let limited = get(running.public, &format!("/{id}"))
        .await
        .expect("public answers");
    assert_eq!(limited.status, 429);
    assert_eq!(
        limited.header("x-webhooks-live-error"),
        Some("rate_limited")
    );
    assert_retry_after_within_period(&limited);

    let first = get(running.public, "/api/v1/instance")
        .await
        .expect("public answers");
    assert_eq!(first.status, 200);
    assert_eq!(first.header("ratelimit-limit"), Some("4"));
    assert_eq!(first.header("ratelimit-remaining"), Some("3"));

    let stream = stream_head(running.public, &id).await;
    assert_eq!(stream.status, 200);
    assert_eq!(stream.header("ratelimit-remaining"), Some("2"));

    for expected in ["1", "0"] {
        let reply = get(running.public, "/api/v1/instance")
            .await
            .expect("public answers");
        assert_eq!(reply.header("ratelimit-remaining"), Some(expected));
    }
    let refused = get(running.public, "/api/v1/instance")
        .await
        .expect("public answers");
    assert_eq!(refused.status, 429);
    assert!(
        refused.body.contains("\"code\":\"rate_limited\""),
        "{}",
        refused.body
    );
    assert_retry_after_within_period(&refused);
    running.stop().await;
}

#[tokio::test]
async fn the_storage_flag_flips_above_110_percent_and_clears_after_eviction() {
    let db = TestDb::new().await;
    let mut holder = PgConnection::connect(db.url())
        .await
        .expect("the holder connects");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(LEADER_LOCK_KEY)
        .execute(&mut holder)
        .await
        .expect("the holder takes the leader lock");
    let oldest = insert_endpoint(db.pool(), 60_000, 3000).await;
    let newer = insert_endpoint(db.pool(), 45_000, 2000).await;
    let live = insert_endpoint(db.pool(), 0, 0).await;
    let running = Running::start(&db, &[("WEBHOOKS_LIVE_STORAGE_CAP_BYTES", "100000B")]).await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    let within = get(running.public, &format!("/{live}"))
        .await
        .expect("public answers");
    assert_eq!(
        within.status, 200,
        "105 percent of the cap still admits captures"
    );

    sqlx::query("UPDATE endpoints SET bytes_used = 51000 WHERE id = $1")
        .bind(&newer)
        .execute(db.pool())
        .await
        .expect("the endpoint grows");
    eventually("captures to be refused with 507", || async {
        get(running.public, &format!("/{live}"))
            .await
            .is_ok_and(|reply| reply.status == 507)
    })
    .await;
    let refused = get(running.public, &format!("/{live}"))
        .await
        .expect("public answers");
    assert_eq!(
        refused.header("x-webhooks-live-error"),
        Some("storage_full")
    );
    assert_eq!(refused.body, "Storage is full. Try again later.");

    holder.close().await.expect("the holder disconnects");
    eventually("eviction to bring the usage down", || async {
        get(running.public, &format!("/{live}"))
            .await
            .is_ok_and(|reply| reply.status == 200)
    })
    .await;
    let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM endpoints ORDER BY id")
        .fetch_all(db.pool())
        .await
        .expect("endpoints read");
    assert!(!remaining.contains(&oldest));
    assert!(remaining.contains(&newer) && remaining.contains(&live));
    running.stop().await;
}

#[tokio::test]
async fn two_servers_share_one_leader_and_shutdown_releases_the_lock() {
    let db = TestDb::new().await;
    let first = Running::start(&db, &[]).await;
    let second = Running::start(&db, &[]).await;

    eventually("a leader to hold the lock", || async {
        advisory_holders(db.pool()).await.len() == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(advisory_holders(db.pool()).await.len(), 1);

    first.stop().await;
    eventually("one replica to keep the lock", || async {
        advisory_holders(db.pool()).await.len() == 1
    })
    .await;
    second.stop().await;
    assert!(advisory_holders(db.pool()).await.is_empty());
}
