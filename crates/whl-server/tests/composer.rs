use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use whl_core::config::Config;
use whl_server::ServerError;
use whl_store::Store;
use whl_testkit::TestDb;

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

#[tokio::test]
async fn a_send_to_the_own_capture_url_becomes_a_capture_without_a_network_hop() {
    let db = TestDb::new().await;
    let running = Running::start(&db).await;
    let endpoint = call(running.public, "POST", "/api/v1/endpoints", "")
        .await
        .json()["id"]
        .as_str()
        .expect("the endpoint has an id")
        .to_owned();
    let request = json!({
        "method": "POST",
        "url": format!("http://127.0.0.1/{endpoint}/hook?x=1"),
        "headers": [{"name": "X-Custom", "value": "v"}],
        "body": {"encoding": "utf8", "data": "hello"},
    });

    let sent = call(running.public, "POST", "/api/v1/send", &request.to_string()).await;

    assert_eq!(sent.status, 200, "{}", sent.body);
    let result = sent.json();
    assert_eq!(result["status"], 200);
    let capture_id = result["capture_id"]
        .as_str()
        .expect("a capture id")
        .to_owned();
    let detail = call(
        running.public,
        "GET",
        &format!("/api/v1/endpoints/{endpoint}/captures/{capture_id}"),
        "",
    )
    .await
    .json();
    assert_eq!(detail["method"], "POST");
    assert_eq!(detail["path"], "/hook");
    assert_eq!(detail["query_string"], "x=1");
    assert_eq!(detail["client_ip"], "127.0.0.1");
    assert_eq!(detail["body"]["data"], "hello");
    let headers = detail["headers"].to_string();
    assert!(headers.contains("x-webhooks-live-composer"), "{headers}");
    assert!(headers.contains("x-custom"), "{headers}");
    running.stop().await;
}

#[tokio::test]
async fn a_self_send_to_a_full_or_unknown_endpoint_reports_the_capture_refusal_as_data() {
    let db = TestDb::new().await;
    let running = Running::start(&db).await;
    let request = json!({
        "method": "GET",
        "url": "http://127.0.0.1/000000000000000000000000/x",
    });

    let sent = call(running.public, "POST", "/api/v1/send", &request.to_string()).await;

    assert_eq!(sent.status, 200, "{}", sent.body);
    assert_eq!(sent.json()["status"], 410);
    assert_eq!(sent.json()["capture_id"], Value::Null);
    running.stop().await;
}

#[tokio::test]
async fn the_api_of_the_instance_itself_cannot_be_called_through_the_composer() {
    let db = TestDb::new().await;
    let running = Running::start(&db).await;
    let request = json!({"method": "GET", "url": "http://127.0.0.1/api/v1/instance"});

    let sent = call(running.public, "POST", "/api/v1/send", &request.to_string()).await;

    assert_eq!(sent.status, 422, "{}", sent.body);
    running.stop().await;
}
