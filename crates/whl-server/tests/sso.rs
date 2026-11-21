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
    request_with(addr, method, path, &[], false).await
}

async fn request_with(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    head_only: bool,
) -> io::Result<Reply> {
    let mut stream = TcpStream::connect(addr).await?;
    let extra: String = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    let text = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n{extra}\r\n"
    );
    stream.write_all(text.as_bytes()).await?;
    let mut raw = Vec::new();
    if head_only {
        let mut chunk = [0_u8; 4096];
        while !raw.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
        }
    } else {
        stream.read_to_end(&mut raw).await?;
    }
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
    wait_for_readyz(admin, true).await;
}

async fn wait_serving(admin: SocketAddr) {
    wait_for_readyz(admin, false).await;
}

async fn wait_for_readyz(admin: SocketAddr, sign_in_too: bool) {
    for _ in 0..500 {
        let ready = request(admin, "GET", "/readyz")
            .await
            .is_ok_and(|reply| reply.status == 200 && (!sign_in_too || reply.body == "ok"));
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
    serve_discovery(listener, &base);
    base
}

fn serve_discovery(listener: TcpListener, base: &str) {
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
    assert!(cookie.starts_with("__Host-whl_login_"));

    let created = request(public, "POST", "/api/v1/endpoints")
        .await
        .expect("public answers");
    assert_eq!(created.status, 401, "creating an endpoint needs a session");
    let body: Value = serde_json::from_str(&created.body).expect("a JSON body");
    assert_eq!(body["error"]["code"], "unauthenticated");

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

async fn closed_issuer() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let base = format!("http://{}", listener.local_addr().expect("an address"));
    (listener, base)
}

fn endpoint_settings() -> whl_store::EndpointSettings {
    whl_store::EndpointSettings {
        title: None,
        response: whl_core::response::ResponseConfig::default(),
        cors: false,
        overflow: whl_store::Overflow::Refuse,
    }
}

#[tokio::test]
async fn sso_mode_with_an_unreachable_issuer_keeps_serving_and_answers_503_on_sign_in() {
    whl_telemetry::metrics::install().expect("the recorder installs");
    let db = TestDb::new().await;
    let (listener, closed) = closed_issuer().await;
    drop(listener);
    let config = try_load_config(db.url(), &sso_env(&closed)).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let endpoint = store
        .create_endpoint(&endpoint_settings(), Duration::from_secs(3600))
        .await
        .expect("the endpoint is created");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_serving(admin).await;

    let ready = request(admin, "GET", "/readyz")
        .await
        .expect("admin answers");
    assert_eq!(ready.status, 200);
    assert!(ready.body.contains("sign-in unavailable"), "{}", ready.body);
    let metrics = request(admin, "GET", "/metrics")
        .await
        .expect("admin answers");
    assert!(metrics.body.contains("webhooks_live_auth_available"));

    for path in ["/auth/login", "/auth/callback?code=x&state=y"] {
        let reply = request(public, "GET", path).await.expect("public answers");
        assert_eq!(reply.status, 503, "{path}");
        assert!(reply.header("retry-after").is_some(), "{path}");
        let body: Value = serde_json::from_str(&reply.body).expect("a JSON body");
        assert_eq!(body["error"]["code"], "auth_unavailable", "{path}");
        assert!(!reply.body.contains("whl-secret"));
    }
    let instance = request(public, "GET", "/api/v1/instance")
        .await
        .expect("public answers");
    assert_eq!(instance.status, 200);
    let captured = request(public, "POST", &format!("/{}", endpoint.id.unpack()))
        .await
        .expect("public answers");
    assert_eq!(captured.status, 200, "the capture path stays public");

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

#[tokio::test]
async fn sign_in_comes_back_once_the_identity_provider_does() {
    let db = TestDb::new().await;
    let (listener, base) = closed_issuer().await;
    let address = listener.local_addr().expect("an address");
    drop(listener);
    let config = try_load_config(db.url(), &sso_env(&base)).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_serving(admin).await;
    let down = request(public, "GET", "/auth/login")
        .await
        .expect("public answers");
    assert_eq!(down.status, 503);

    let listener = TcpListener::bind(address)
        .await
        .expect("the port is free again");
    serve_discovery(listener, &base);
    let mut status = 0;
    for _ in 0..100 {
        status = request(public, "GET", "/auth/login")
            .await
            .expect("public answers")
            .status;
        if status == 302 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, 302, "discovery was retried in the background");
    let ready = request(admin, "GET", "/readyz")
        .await
        .expect("admin answers");
    assert_eq!((ready.status, ready.body.as_str()), (200, "ok"));

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

async fn session_for(store: &Store, tag: &str, email: &str) -> (whl_store::Uuid, String) {
    use sha2::{Digest, Sha256};
    let user = store
        .upsert_user(&whl_store::VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: format!("subject-{tag}"),
            email: email.to_owned(),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored");
    let token = format!("{tag:0<43}");
    store
        .create_session(
            Sha256::digest(token.as_bytes()).as_slice(),
            user.id,
            Duration::from_secs(3600),
        )
        .await
        .expect("the session is stored");
    (user.id, format!("__Host-whl_session={token}"))
}

#[tokio::test]
async fn the_stream_follows_the_same_role_rules_as_the_api() {
    let db = TestDb::new().await;
    let issuer = discovery_stub().await;
    let config = try_load_config(db.url(), &sso_env(&issuer)).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let (owner_id, owner) = session_for(&store, "own", "owner@example.test").await;
    let (_, viewer) = session_for(&store, "vie", "viewer@example.test").await;
    let (_, other) = session_for(&store, "oth", "other@example.test").await;
    let retention = Duration::from_secs(3600);
    let endpoint = store
        .create_endpoint_owned(Some(owner_id), &endpoint_settings(), retention)
        .await
        .expect("the endpoint is created");
    let id = endpoint.id.unpack().to_owned();
    store
        .set_member(
            &endpoint.id,
            retention,
            "viewer@example.test",
            whl_store::GrantedRole::Viewer,
        )
        .await
        .expect("the member is stored");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_ready(admin).await;

    let stream = format!("/api/v1/endpoints/{id}/stream");
    let unknown = "/api/v1/endpoints/000000000000000000000000/stream";
    let anonymous = request_with(public, "GET", &stream, &[], true)
        .await
        .expect("public answers");
    assert_eq!(anonymous.status, 410);
    let unknown_reply = request_with(public, "GET", unknown, &[], true)
        .await
        .expect("public answers");
    assert_eq!(unknown_reply.status, 410);
    assert_eq!(anonymous.body, unknown_reply.body);
    let stranger = request_with(public, "GET", &stream, &[("Cookie", &other)], true)
        .await
        .expect("public answers");
    assert_eq!(stranger.status, 410);
    assert_eq!(stranger.body, anonymous.body);
    let not_an_id = request_with(public, "GET", "/api/v1/endpoints/zz/stream", &[], true)
        .await
        .expect("public answers");
    assert_eq!(not_an_id.status, 404);

    for cookie in [&viewer, &owner] {
        let reply = request_with(public, "GET", &stream, &[("Cookie", cookie)], true)
            .await
            .expect("public answers");
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.header("content-type").as_deref(),
            Some("text/event-stream")
        );
    }

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

#[tokio::test]
async fn the_sign_in_routes_share_the_per_ip_api_limiter() {
    let db = TestDb::new().await;
    let issuer = discovery_stub().await;
    let mut env = sso_env(&issuer);
    env.push(("WEBHOOKS_LIVE_RATE_API_IP", "1/m:2"));
    let config = try_load_config(db.url(), &env).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_ready(admin).await;

    let first = request(public, "GET", "/auth/login")
        .await
        .expect("answers");
    let second = request(public, "GET", "/auth/callback?code=x&state=y")
        .await
        .expect("answers");
    let third = request(public, "GET", "/auth/login")
        .await
        .expect("answers");
    assert_eq!((first.status, second.status, third.status), (302, 302, 429));
    assert!(third.header("retry-after").is_some());
    let body: Value = serde_json::from_str(&third.body).expect("a JSON body");
    assert_eq!(body["error"]["code"], "rate_limited");

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
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

fn mixed_env(issuer: &str) -> Vec<(&str, &str)> {
    let mut env = sso_env(issuer);
    env[0] = ("WEBHOOKS_LIVE_AUTH_MODE", "mixed");
    env
}

#[tokio::test]
async fn mixed_mode_serves_anonymous_callers_and_gates_owned_endpoints_over_a_real_socket() {
    let db = TestDb::new().await;
    let issuer = discovery_stub().await;
    let config = try_load_config(db.url(), &mixed_env(&issuer)).expect("the configuration loads");
    let store = Store::connect(&config.database).await.expect("connects");
    let (owner_id, owner) = session_for(&store, "own", "owner@example.test").await;
    let retention = Duration::from_secs(3600);
    let owned = store
        .create_endpoint_owned(Some(owner_id), &endpoint_settings(), retention)
        .await
        .expect("the endpoint is created");
    let owned_id = owned.id.unpack().to_owned();
    let bound = whl_server::bind(&config).await.expect("binds");
    let (public, admin) = (bound.public_addr(), bound.admin_addr());
    let (trigger, stop) = oneshot::channel::<()>();
    let server = tokio::spawn(bound.serve(store, async {
        let _ = stop.await;
    }));
    wait_ready(admin).await;

    let instance = request(public, "GET", "/api/v1/instance")
        .await
        .expect("public answers");
    let instance: Value = serde_json::from_str(&instance.body).expect("a JSON body");
    assert_eq!(instance["auth_mode"], "mixed");
    assert_eq!(instance["auth"]["sign_in_available"], true);

    let created = request(public, "POST", "/api/v1/endpoints")
        .await
        .expect("public answers");
    assert_eq!(created.status, 201, "anonymous creation keeps working");
    let created: Value = serde_json::from_str(&created.body).expect("a JSON body");
    let anonymous_id = created["id"].as_str().expect("an id").to_owned();
    let recorded: Option<String> =
        sqlx::query_scalar("SELECT creator_net::text FROM endpoints WHERE id = $1")
            .bind(&anonymous_id)
            .fetch_one(db.pool())
            .await
            .expect("the endpoint row exists");
    assert_eq!(recorded.as_deref(), Some("127.0.0.0/24"));

    let login = request(public, "GET", "/auth/login")
        .await
        .expect("public answers");
    assert_eq!(login.status, 302);

    let anonymous_stream = format!("/api/v1/endpoints/{anonymous_id}/stream");
    let owned_stream = format!("/api/v1/endpoints/{owned_id}/stream");
    let open_door = request_with(public, "GET", &anonymous_stream, &[], true)
        .await
        .expect("public answers");
    let closed_door = request_with(public, "GET", &owned_stream, &[], true)
        .await
        .expect("public answers");
    let owner_door = request_with(public, "GET", &owned_stream, &[("Cookie", &owner)], true)
        .await
        .expect("public answers");
    assert_eq!(open_door.status, 200);
    assert_eq!(closed_door.status, 410);
    assert_eq!(owner_door.status, 200);

    let capture = request(public, "POST", &format!("/{owned_id}"))
        .await
        .expect("public answers");
    assert_eq!(
        capture.status, 200,
        "capture URLs stay public on owned endpoints"
    );

    let _ = trigger.send(());
    server.await.expect("joins").expect("stops cleanly");
}

#[tokio::test]
async fn mixed_mode_without_oidc_keys_is_rejected_by_the_configuration() {
    let db = TestDb::new().await;
    let error = try_load_config(db.url(), &[("WEBHOOKS_LIVE_AUTH_MODE", "mixed")])
        .expect_err("the configuration is incomplete");
    let text = error.to_string();
    for key in ["OIDC_ISSUER", "OIDC_CLIENT_ID", "OIDC_CLIENT_SECRET"] {
        assert!(text.contains(key), "{text}");
    }
    assert!(text.contains("mixed"), "{text}");
}
