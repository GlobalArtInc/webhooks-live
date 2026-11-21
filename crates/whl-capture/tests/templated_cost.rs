mod common;
mod served;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{Harness, PUBLIC_URL};
use serde_json::json;
use served::{Configured, endpoint_configured, make_templated, set_var};
use sqlx::postgres::PgPoolOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;
use whl_capture::{CaptureSettings, CaptureState};
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

async fn templated_with(harness: &Harness, body: &str) -> EndpointId {
    let id = endpoint_configured(
        harness,
        Configured {
            body: body.as_bytes().to_vec(),
            ..Configured::default()
        },
    )
    .await;
    make_templated(harness, &id, None).await;
    id
}

const SSL_REQUEST_CODE: u32 = 80_877_103;
const BIND: u8 = b'B';
const SIMPLE_QUERY: u8 = b'Q';

async fn relay(
    mut client: TcpStream,
    mut server: TcpStream,
    statements: Arc<AtomicUsize>,
) -> io::Result<()> {
    loop {
        let mut head = [0_u8; 8];
        client.read_exact(&mut head).await?;
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let code = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
        if code == SSL_REQUEST_CODE {
            client.write_all(b"N").await?;
            continue;
        }
        let mut rest = vec![0_u8; length.saturating_sub(head.len())];
        client.read_exact(&mut rest).await?;
        server.write_all(&head).await?;
        server.write_all(&rest).await?;
        break;
    }
    let (mut client_read, mut client_write) = client.into_split();
    let (mut server_read, mut server_write) = server.into_split();
    let back = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut server_read, &mut client_write).await;
    });
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    let outcome = loop {
        let read = match client_read.read(&mut chunk).await {
            Ok(0) | Err(_) => break Ok(()),
            Ok(read) => read,
        };
        pending.extend_from_slice(&chunk[..read]);
        while pending.len() >= 5 {
            let length =
                u32::from_be_bytes([pending[1], pending[2], pending[3], pending[4]]) as usize;
            let total = length + 1;
            if pending.len() < total {
                break;
            }
            if pending[0] == BIND || pending[0] == SIMPLE_QUERY {
                statements.fetch_add(1, Ordering::SeqCst);
            }
            if let Err(error) = server_write.write_all(&pending[..total]).await {
                back.abort();
                return Err(error);
            }
            pending.drain(..total);
        }
    };
    back.abort();
    outcome
}

async fn counted_harness() -> (Harness, Arc<AtomicUsize>) {
    let db = TestDb::new().await;
    let target = Url::parse(db.url()).expect("the database URL parses");
    let host = target.host_str().expect("a host").to_owned();
    let port = target.port().expect("a port");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let local_port = listener.local_addr().expect("local address").port();
    let statements = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&statements);
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            let Ok(server) = TcpStream::connect((host.as_str(), port)).await else {
                return;
            };
            tokio::spawn(relay(client, server, Arc::clone(&counted)));
        }
    });
    let mut proxied = target;
    proxied
        .set_host(Some("127.0.0.1"))
        .expect("the host is set");
    proxied.set_port(Some(local_port)).expect("the port is set");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(proxied.as_str())
        .await
        .expect("the proxied pool connects");
    let state = CaptureState::new(
        Store::from_pool(pool),
        CaptureSettings {
            limits: common::default_limits(),
            trusted_proxies: Vec::new(),
            public_url: PUBLIC_URL.to_owned(),
            max_delayed: 2000,
        },
    )
    .expect("settings are valid");
    let router = whl_capture::router(state.clone());
    (Harness { db, state, router }, statements)
}

async fn statements_for(harness: &Harness, statements: &AtomicUsize, id: &EndpointId) -> usize {
    let before = statements.load(Ordering::SeqCst);
    harness.get(&format!("/{id}")).await;
    statements.load(Ordering::SeqCst) - before
}

#[tokio::test]
async fn templates_that_reference_no_variables_cost_no_extra_queries() {
    let (harness, statements) = counted_harness().await;
    let plain = endpoint_configured(
        &harness,
        Configured {
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let no_refs = templated_with(&harness, "{{ request.method }} {{ capture.seq }}").await;
    let one_counter = templated_with(&harness, "{{ counter.hits }}").await;
    let several = templated_with(
        &harness,
        "{{ counter.a }}{{ counter.b }}{{ store.c }}{{ store.d.e }}",
    )
    .await;
    for id in [&plain, &no_refs, &one_counter, &several] {
        harness.get(&format!("/{id}")).await;
    }

    let baseline = statements_for(&harness, &statements, &plain).await;
    let templated = statements_for(&harness, &statements, &no_refs).await;
    let with_counter = statements_for(&harness, &statements, &one_counter).await;
    let with_everything = statements_for(&harness, &statements, &several).await;

    assert!(baseline > 0);
    assert_eq!(templated, baseline);
    assert_eq!(with_counter, baseline + 1);
    assert_eq!(with_everything, baseline + 1);
}

#[tokio::test]
async fn a_counter_reads_the_committed_value_and_a_missing_one_renders_zero() {
    let harness = Harness::new().await;
    let id = templated_with(
        &harness,
        "[{{ counter.hits }}][{{ counter.none }}][{{ store.text }}]",
    )
    .await;
    let store = Store::from_pool(harness.db.pool().clone());
    set_var(&harness, &id, "text", json!("not a counter")).await;
    for _ in 0..3 {
        store
            .increment_counter(&id, RETENTION, "hits")
            .await
            .expect("the counter increments");
    }

    let reply = harness.get(&format!("/{id}")).await;
    store
        .increment_counter(&id, RETENTION, "hits")
        .await
        .expect("the counter increments");
    let again = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.body, "[3][0][not a counter]");
    assert_eq!(again.body, "[4][0][not a counter]");
    assert!(reply.header("x-webhooks-live-template").is_none());
}

#[tokio::test]
async fn a_counter_name_holding_text_renders_as_a_missing_counter_zero() {
    let harness = Harness::new().await;
    let id = templated_with(&harness, "[{{ counter.label }}]").await;
    set_var(&harness, &id, "label", json!("text")).await;

    let reply = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.body, "[0]");
}
