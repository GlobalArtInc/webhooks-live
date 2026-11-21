use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use whl_core::config::Config;

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
        .expect("response has a header block");
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("response has a status code");
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
fn load_config() -> Config {
    let mut result = None;
    Jail::expect_with(|jail| {
        let vars = [
            ("WEBHOOKS_LIVE_PUBLIC_URL", "http://127.0.0.1"),
            ("WEBHOOKS_LIVE_DATABASE_URL", "postgres://u:p@localhost/db"),
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

#[tokio::test]
async fn serves_probes_and_drains_when_shutdown_fires() {
    let bound = whl_server::bind(&load_config())
        .await
        .expect("ephemeral ports bind");
    let public = bound.public_addr();
    let admin = bound.admin_addr();

    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(async {
        let _ = stop.await;
    }));

    let healthz = get(admin, "/healthz").await.expect("admin answers");
    assert_eq!((healthz.status, healthz.body.as_str()), (200, "ok"));

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
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        whl_server::run(load_config(), async {}),
    )
    .await
    .expect("run returns within the grace period");

    result.expect("run exits cleanly");
}
