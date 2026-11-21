mod support;

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use support::{RETENTION, TestApp, capture_payload, insert_endpoint, notify};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use whl_live::{ListenerSettings, StreamSettings};
use whl_store::Store;
use whl_testkit::TestDb;

const LISTEN_COMMAND: &[u8] = b"LISTEN";
const RELAY_CHUNK: usize = 8192;

#[derive(Default)]
struct Pairing {
    listener: AtomicBool,
    frozen: AtomicBool,
}

struct Proxy {
    port: u16,
    pairings: Arc<Mutex<Vec<Arc<Pairing>>>>,
}

impl Proxy {
    async fn start(host: String, port: u16) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("an ephemeral port binds");
        let local = socket
            .local_addr()
            .expect("the socket has an address")
            .port();
        let pairings: Arc<Mutex<Vec<Arc<Pairing>>>> = Arc::default();
        let registry = Arc::clone(&pairings);
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = socket.accept().await else {
                    return;
                };
                let upstream = TcpStream::connect((host.as_str(), port))
                    .await
                    .expect("the database accepts");
                let pairing = Arc::new(Pairing::default());
                registry
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Arc::clone(&pairing));
                let (client_read, client_write) = client.into_split();
                let (upstream_read, upstream_write) = upstream.into_split();
                tokio::spawn(relay(
                    client_read,
                    upstream_write,
                    true,
                    Arc::clone(&pairing),
                ));
                tokio::spawn(relay(upstream_read, client_write, false, pairing));
            }
        });
        Self {
            port: local,
            pairings,
        }
    }

    fn freeze_current_listener_connections(&self) {
        let pairings = self.pairings.lock().unwrap_or_else(PoisonError::into_inner);
        for pairing in pairings.iter() {
            if pairing.listener.load(Ordering::SeqCst) {
                pairing.frozen.store(true, Ordering::SeqCst);
            }
        }
    }
}

async fn relay(mut from: OwnedReadHalf, mut to: OwnedWriteHalf, scan: bool, pairing: Arc<Pairing>) {
    let mut buffer = [0_u8; RELAY_CHUNK];
    loop {
        let read = match from.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let chunk = &buffer[..read];
        if scan
            && chunk
                .windows(LISTEN_COMMAND.len())
                .any(|window| window == LISTEN_COMMAND)
        {
            pairing.listener.store(true, Ordering::SeqCst);
        }
        if pairing.frozen.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if to.write_all(chunk).await.is_err() {
            return;
        }
    }
}

#[tokio::test]
async fn a_frozen_listener_connection_is_detected_by_the_heartbeat_and_resynced() {
    let db = TestDb::new().await;
    let direct = PgConnectOptions::from_str(db.url()).expect("the test url parses");
    let proxy = Proxy::start(direct.get_host().to_owned(), direct.get_port()).await;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(direct.clone().host("127.0.0.1").port(proxy.port))
        .await
        .expect("the pool connects through the proxy");
    let beat = Duration::from_secs(1);
    let listening = ListenerSettings {
        heartbeat_interval: beat,
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(200),
        ..ListenerSettings::default()
    };
    let app = TestApp::start_on(
        Store::from_pool(pool),
        256,
        StreamSettings::new(RETENTION),
        listening,
    )
    .await;
    let endpoint = insert_endpoint(db.pool()).await;
    let mut client = app.open(&endpoint).await;
    client.expect_event("ready").await;
    notify(db.pool(), &capture_payload(&endpoint, 1)).await;
    client.expect_capture(1).await;

    tokio::time::sleep(beat * 4).await;
    assert_eq!(app.listener.establishments(), 1);

    let frozen_at = Instant::now();
    proxy.freeze_current_listener_connections();
    let resync = client.expect_event("resync").await;

    assert_eq!(
        resync.json(),
        serde_json::json!({"reason": "listener_reconnect"})
    );
    assert!(frozen_at.elapsed() < beat * 2 + Duration::from_secs(10));
    app.listener
        .wait_for_establishments(2)
        .await
        .expect("the listener reconnects");
    notify(db.pool(), &capture_payload(&endpoint, 2)).await;
    client.expect_capture(2).await;
}
