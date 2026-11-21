use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use figment::Jail;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use whl_core::config::Config;
use whl_server::ServerError;
use whl_store::Store;
use whl_testkit::TestDb;

struct Target {
    addr: SocketAddr,
    received: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Target {
    async fn start(hold: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the target binds");
        let addr = listener.local_addr().expect("the address is known");
        let received = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn({
            let received = Arc::clone(&received);
            async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let received = Arc::clone(&received);
                    tokio::spawn(async move {
                        let mut buffer = vec![0_u8; 8192];
                        let mut seen = Vec::new();
                        while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                            match stream.read(&mut buffer).await {
                                Ok(0) | Err(_) => return,
                                Ok(read) => seen.extend_from_slice(&buffer[..read]),
                            }
                        }
                        received.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(hold).await;
                        let _ = stream
                            .write_all(
                                b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\n\r\nok",
                            )
                            .await;
                    });
                }
            }
        });
        Self {
            addr,
            received,
            task,
        }
    }

    fn received(&self) -> usize {
        self.received.load(Ordering::SeqCst)
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Reply {
    status: u16,
    body: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).expect("the body is JSON")
    }
}

async fn call(addr: SocketAddr, method: &str, path: &str, body: &str) -> Reply {
    let mut stream = TcpStream::connect(addr).await.expect("the server accepts");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("the request is sent");
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .expect("the response is read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").expect("a response head");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status code");
    Reply {
        status,
        body: body.to_owned(),
    }
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
            ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "100ms"),
            ("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "10s"),
            ("WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS", "127.0.0.1/32"),
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

struct Running {
    public: SocketAddr,
    trigger: oneshot::Sender<()>,
    server: JoinHandle<Result<(), ServerError>>,
}

impl Running {
    async fn start(db: &TestDb) -> Self {
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
        for _ in 0..500 {
            if call(admin, "GET", "/readyz", "").await.status == 200 {
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
        tokio::time::timeout(Duration::from_secs(15), self.server)
            .await
            .expect("serve returns within the grace period")
            .expect("server task joins")
            .expect("serve returns Ok after shutdown");
    }
}

async fn endpoint_with_rule(public: SocketAddr, target: SocketAddr) -> String {
    let endpoint = call(public, "POST", "/api/v1/endpoints", "").await.json()["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    let rule = call(
        public,
        "POST",
        &format!("/api/v1/endpoints/{endpoint}/forward-rules"),
        &format!(r#"{{"target_url":"http://{target}/hook"}}"#),
    )
    .await;
    assert_eq!(rule.status, 201, "{}", rule.body);
    endpoint
}

async fn wait_for_forward_state(public: SocketAddr, endpoint: &str, capture: &str, count: usize) {
    for _ in 0..500 {
        let detail = call(
            public,
            "GET",
            &format!("/api/v1/endpoints/{endpoint}/captures/{capture}"),
            "",
        )
        .await
        .json();
        let forwards = detail["forwards"].as_array().cloned().unwrap_or_default();
        let finished = forwards
            .iter()
            .filter(|job| job["state"] == "succeeded")
            .count();
        if finished >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the forward jobs never succeeded");
}

async fn newest_capture_id(public: SocketAddr, endpoint: &str) -> String {
    call(
        public,
        "GET",
        &format!("/api/v1/endpoints/{endpoint}/captures/latest"),
        "",
    )
    .await
    .json()["id"]
        .as_str()
        .expect("the capture has an id")
        .to_owned()
}

#[tokio::test]
async fn a_capture_is_forwarded_and_a_resend_runs_through_the_whole_stack() {
    let db = TestDb::new().await;
    let target = Target::start(Duration::ZERO).await;
    let running = Running::start(&db).await;
    let endpoint = endpoint_with_rule(running.public, target.addr).await;

    let sent = call(
        running.public,
        "POST",
        &format!("/{endpoint}/orders"),
        r#"{"a":1}"#,
    )
    .await;
    assert_eq!(sent.status, 200);
    let capture = newest_capture_id(running.public, &endpoint).await;
    wait_for_forward_state(running.public, &endpoint, &capture, 1).await;
    assert_eq!(target.received(), 1);

    let resent = call(
        running.public,
        "POST",
        &format!("/api/v1/endpoints/{endpoint}/captures/{capture}/forward"),
        &format!(r#"{{"target_url":"http://{}/again"}}"#, target.addr),
    )
    .await;
    assert_eq!(resent.status, 202, "{}", resent.body);
    wait_for_forward_state(running.public, &endpoint, &capture, 2).await;
    assert_eq!(target.received(), 2);

    let detail = call(
        running.public,
        "GET",
        &format!("/api/v1/endpoints/{endpoint}/captures/{capture}"),
        "",
    )
    .await
    .json();
    assert_eq!(detail["forward_state"], "succeeded");
    assert_eq!(detail["forwards"][0]["kind"], "manual");
    assert_eq!(detail["forwards"][1]["kind"], "rule");
    assert_eq!(detail["forwards"][1]["attempts"][0]["status"], 200);
    assert_eq!(
        detail["forwards"][1]["attempts"][0]["response_body"]["data"],
        "ok"
    );
    running.stop().await;
}

#[tokio::test]
async fn shutdown_waits_for_the_forward_attempt_in_flight_and_records_it() {
    let db = TestDb::new().await;
    let target = Target::start(Duration::from_millis(700)).await;
    let running = Running::start(&db).await;
    let endpoint = endpoint_with_rule(running.public, target.addr).await;
    call(running.public, "POST", &format!("/{endpoint}"), "x").await;

    for _ in 0..500 {
        if target.received() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(target.received(), 1);
    running.stop().await;

    let states: Vec<String> = sqlx::query_scalar("SELECT state FROM forward_jobs")
        .fetch_all(db.pool())
        .await
        .expect("the jobs are readable");
    assert_eq!(states, vec!["succeeded".to_owned()]);
    let attempts: i64 = sqlx::query_scalar("SELECT count(*) FROM forward_attempts")
        .fetch_one(db.pool())
        .await
        .expect("the attempts are countable");
    assert_eq!(attempts, 1);
}
