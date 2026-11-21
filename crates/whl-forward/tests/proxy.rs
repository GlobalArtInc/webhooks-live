mod support;

use std::process::Command;

use support::{Reply, TestServer, client, get, loopback_config};

const PROBE_TARGET_VARIABLE: &str = "WHL_FORWARD_PROXY_PROBE_TARGET";
const PROXY_VARIABLES: [&str; 6] = [
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];

#[tokio::test]
async fn ignores_system_proxy_environment_variables() {
    let config = loopback_config();
    if let Ok(target) = std::env::var(PROBE_TARGET_VARIABLE) {
        let response = client(config.clone())
            .send(get(&target, &config))
            .await
            .unwrap();
        assert_eq!(response.body.as_ref(), b"direct");
        return;
    }

    let target = TestServer::start("127.0.0.1:0", |_| Reply::ok("direct")).await;
    let proxy = TestServer::start("127.0.0.1:0", |_| Reply::ok("proxied")).await;
    let proxy_url = format!("http://{}", proxy.addr);
    let target_url = target.url("/probe");
    let test_binary = std::env::current_exe().unwrap();
    let output = tokio::task::spawn_blocking(move || {
        let mut command = Command::new(test_binary);
        command
            .args(["--exact", "ignores_system_proxy_environment_variables"])
            .env(PROBE_TARGET_VARIABLE, target_url)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy");
        for variable in PROXY_VARIABLES {
            command.env(variable, &proxy_url);
        }
        command.output()
    })
    .await
    .unwrap()
    .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(target.seen().len(), 1);
    assert!(proxy.seen().is_empty());
    assert_eq!(proxy.connections(), 0);
}
