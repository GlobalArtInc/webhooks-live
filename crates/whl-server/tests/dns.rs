use std::net::SocketAddr;
use std::time::Duration;

use figment::Jail;
use tokio::net::UdpSocket;
use tokio::sync::oneshot;
use whl_core::config::Config;
use whl_core::response::ResponseConfig;
use whl_server::ServerError;
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const ZONE: &str = "dns.test.example";
const OUTER_DEADLINE: Duration = Duration::from_secs(30);
const RETENTION: Duration = Duration::from_secs(7 * 86_400);

#[allow(clippy::result_large_err)]
fn load_config(database_url: &str, with_dns: bool) -> Config {
    let mut result = None;
    Jail::expect_with(|jail| {
        let mut vars = vec![
            ("WEBHOOKS_LIVE_PUBLIC_URL", "http://127.0.0.1"),
            ("WEBHOOKS_LIVE_DATABASE_URL", database_url),
            ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_ADMIN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "1s"),
            ("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "60s"),
        ];
        if with_dns {
            vars.extend([
                ("WEBHOOKS_LIVE_DNS_LISTEN_ADDR", "127.0.0.1:0"),
                ("WEBHOOKS_LIVE_DNS_ZONE", ZONE),
                ("WEBHOOKS_LIVE_DNS_NS_NAME", "ns.test.example"),
                ("WEBHOOKS_LIVE_DNS_SOA_EMAIL", "hostmaster@test.example"),
                ("WEBHOOKS_LIVE_DNS_ANSWER_A", "192.0.2.7"),
            ]);
        }
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

fn a_query(name: &str) -> Vec<u8> {
    let mut packet = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        packet.push(u8::try_from(label.len()).expect("short label"));
        packet.extend_from_slice(label.as_bytes());
    }
    packet.extend_from_slice(&[0, 0, 1, 0, 1]);
    packet
}

async fn ask(server: SocketAddr, packet: &[u8]) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client binds");
    socket.send_to(packet, server).await.expect("datagram sent");
    let mut buffer = vec![0_u8; 1024];
    match tokio::time::timeout(Duration::from_millis(500), socket.recv_from(&mut buffer)).await {
        Ok(Ok((length, _))) => Some(buffer[..length].to_vec()),
        _ => None,
    }
}

#[tokio::test]
async fn the_server_answers_and_captures_dns_queries_and_stops_with_the_rest() {
    let db = TestDb::new().await;
    let config = load_config(db.url(), true);
    let store = Store::connect(&config.database)
        .await
        .expect("the test database accepts connections");
    let record = store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                templating: Default::default(),
                cors: false,
                overflow: Overflow::Refuse,
                tags: Vec::new(),
                verifier: None,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    store
        .set_dns_enabled(&record.id, true)
        .await
        .expect("the flag is written");
    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");
    let dns = bound.dns_addrs().expect("DNS capture is configured");
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));

    let mut reply = None;
    for _ in 0..200 {
        reply = ask(dns.udp, &a_query(&format!("{}.{ZONE}", record.id))).await;
        if reply.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let reply = reply.expect("the DNS responder answers once the server is ready");
    assert_eq!(reply[3] & 0x0f, 0);
    assert_eq!(&reply[6..8], &[0, 1]);
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM captures WHERE kind = 'dns'")
        .fetch_one(db.pool())
        .await
        .expect("captures are countable");
    assert!(stored >= 1, "{stored} captures");
    trigger.send(()).expect("server waits for the shutdown");
    tokio::time::timeout(OUTER_DEADLINE, server)
        .await
        .expect("run returns within the grace period")
        .expect("server task joins")
        .expect("serve returns Ok after shutdown");
}

#[tokio::test]
async fn without_dns_keys_no_dns_listener_exists() {
    let db = TestDb::new().await;
    let config = load_config(db.url(), false);

    let bound = whl_server::bind(&config)
        .await
        .expect("ephemeral ports bind");

    assert!(bound.dns_addrs().is_none());
}

#[tokio::test]
async fn an_invalid_zone_stops_the_start_with_a_named_error() {
    let db = TestDb::new().await;
    let mut config = load_config(db.url(), true);
    config.dns.zone = Some("bad zone!".to_owned());

    let error = whl_server::bind(&config)
        .await
        .err()
        .expect("the zone is refused");

    assert!(matches!(error, ServerError::Dns(_)), "{error:?}");
}
