use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::routing::get;
use figment::Jail;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use whl_core::config::{Config, ConfigError};
use whl_server::ServerError;
use whl_store::Store;
use whl_testkit::TestDb;

struct Reply {
    status: u16,
    head: String,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    }
}

async fn request(addr: SocketAddr, method: &str, path: &str) -> io::Result<Reply> {
    let mut stream = TcpStream::connect(addr).await?;
    let text = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(text.as_bytes()).await?;
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
    Ok(Reply {
        status,
        head: head.to_owned(),
        body: body.to_owned(),
    })
}

fn try_load_config(database_url: &str, extra: &[(&str, &str)]) -> Result<Config, ConfigError> {
    let mut result = None;
    #[allow(clippy::result_large_err)]
    Jail::expect_with(|jail| {
        let base = [
            ("WEBHOOKS_LIVE_PUBLIC_URL", "https://whl.test"),
            ("WEBHOOKS_LIVE_DATABASE_URL", database_url),
            ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_ADMIN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_SHUTDOWN_DRAIN", "1s"),
            ("WEBHOOKS_LIVE_SHUTDOWN_GRACE", "5s"),
        ];
        for (key, value) in base.iter().chain(extra.iter()) {
            jail.set_env(key, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result.expect("the jail closure always runs")
}

async fn wait_ready(admin: SocketAddr) {
    for _ in 0..500 {
        let ready = request(admin, "GET", "/readyz")
            .await
            .is_ok_and(|reply| reply.status == 200);
        if ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("readyz never answered 200");
}

async fn discovery_stub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the stub binds");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    let document = json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
    });
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || {
                let document = document.clone();
                async move { Json(document) }
            }),
        )
        .route("/jwks", get(|| async { Json(json!({ "keys": [] })) }));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    base
}

fn sso_env(issuer: &str) -> Vec<(&str, &str)> {
    vec![
        ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
        ("WEBHOOKS_LIVE_OIDC_ISSUER", issuer),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "whl-client"),
        ("WEBHOOKS_LIVE_OIDC_CLIENT_SECRET", "whl-secret"),
    ]
}

#[tokio::test]
async fn open_mode_answers_404_not_available_on_every_auth_route() {
    let db = TestDb::new().await;
    let config = try_load_config(db.url(), &[]).expect("the open configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_ready(admin).await;

    for (method, path) in [
        ("GET", "/auth/login"),
        ("GET", "/auth/callback?code=x&state=y"),
        ("POST", "/auth/logout"),
        ("GET", "/auth/anything/else"),
    ] {
        let reply = request(public, method, path).await.expect("public answers");
        assert_eq!(reply.status, 404, "{method} {path}");
        let body: Value = serde_json::from_str(&reply.body).expect("a JSON body");
        assert_eq!(body["error"]["code"], "not_available_in_mode");
    }

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

#[tokio::test]
async fn sso_mode_mounts_the_auth_router_and_keeps_the_probes_alive() {
    let db = TestDb::new().await;
    let issuer = discovery_stub().await;
    let config = try_load_config(db.url(), &sso_env(&issuer)).expect("the sso configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));

    let healthz = request(admin, "GET", "/healthz")
        .await
        .expect("admin answers");
    assert_eq!(healthz.status, 200);
    wait_ready(admin).await;

    let login = request(public, "GET", "/auth/login?return_to=/e/x")
        .await
        .expect("public answers");
    assert_eq!(login.status, 302);
    let location = login.header("location").expect("a location header");
    assert!(
        location.starts_with(&format!("{issuer}/authorize?")),
        "{location}"
    );
    assert!(location.contains("code_challenge_method=S256"));
    let cookie = login.header("set-cookie").expect("a login cookie");
    assert!(cookie.starts_with("__Host-whl_login="));

    let created = request(public, "POST", "/api/v1/endpoints")
        .await
        .expect("public answers");
    assert_eq!(
        created.status, 201,
        "no session cookie means no origin check"
    );

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

#[tokio::test]
async fn sso_mode_with_an_unreachable_issuer_fails_startup_with_a_clear_error() {
    let db = TestDb::new().await;
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        format!("http://{}", listener.local_addr().expect("an address"))
    };
    let config = try_load_config(db.url(), &sso_env(&closed)).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let bound = whl_server::bind(&config).await.expect("binds");

    let outcome = bound.serve(store, std::future::pending()).await;
    match outcome {
        Err(ServerError::Auth(error)) => {
            let text = error.to_string();
            assert!(text.contains("OIDC discovery"), "{text}");
            assert!(text.contains(&closed), "{text}");
            assert!(!text.contains("whl-secret"), "{text}");
        }
        other => panic!("expected an authentication startup error, got {other:?}"),
    }
}

#[tokio::test]
async fn sso_mode_without_oidc_keys_is_rejected_by_the_configuration() {
    let db = TestDb::new().await;
    let error = try_load_config(db.url(), &[("WEBHOOKS_LIVE_AUTH_MODE", "sso")])
        .expect_err("the configuration is incomplete");
    let text = error.to_string();
    for key in ["OIDC_ISSUER", "OIDC_CLIENT_ID", "OIDC_CLIENT_SECRET"] {
        assert!(text.contains(key), "{text}");
    }
}
