#![cfg(unix)]

mod support;

use std::path::Path;

use support::{
    CaptureSpec, Cli, ENDPOINT, Fake, FakeOptions, Scratch, Target, closed_port, eventually,
};

const TOKEN: &str = "whl_integration_token";

fn forward_args(fake: &Fake, target: &str, state: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec![
        "forward".to_owned(),
        ENDPOINT.to_owned(),
        "--server".to_owned(),
        fake.url(),
        "--to".to_owned(),
        target.to_owned(),
        "--state-file".to_owned(),
        state.to_string_lossy().into_owned(),
    ];
    args.extend(extra.iter().map(|argument| (*argument).to_owned()));
    args
}

fn last_seq(state: &std::path::Path) -> u64 {
    let text = std::fs::read_to_string(state).expect("state file");
    let value: serde_json::Value = serde_json::from_str(&text).expect("state json");
    value["last_seq"].as_u64().expect("last_seq")
}

fn bodies(target: &Target) -> Vec<String> {
    target
        .received()
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .collect()
}

#[tokio::test]
async fn replays_a_live_capture_byte_exact_through_the_body_route() {
    let fake = Fake::start(FakeOptions::default()).await;
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("/hooks");
    let args = forward_args(&fake, &target_url, &state, &[]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("connected").await;

    let payload = [0xff, 0x00, 0x80, b'\r', b'\n', 0xc3, b'{'];
    fake.push(
        CaptureSpec::new("POST", "/orders/42")
            .query("a=1&b=%20x&a=2")
            .header("Host", "webhooks.live")
            .header("Content-Type", "application/octet-stream")
            .header("X-Signature", "sig-1")
            .header("X-Dup", "a")
            .header("X-Dup", "b")
            .header("Connection", "X-Private")
            .header("X-Private", "1")
            .header("Keep-Alive", "timeout=5")
            .header("Content-Length", "7")
            .body(&payload),
    );
    cli.wait_for("#1 POST /orders/42?a=1&b=%20x&a=2 -> 202")
        .await;

    let received = target.received();
    assert_eq!(received.len(), 1);
    let request = &received[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.target, "/hooks/orders/42?a=1&b=%20x&a=2");
    assert_eq!(request.body, payload);
    assert_eq!(request.header("x-signature"), Some(&b"sig-1"[..]));
    assert_eq!(request.header_values("x-dup"), vec![&b"a"[..], &b"b"[..]]);
    assert_eq!(
        request.header("content-type"),
        Some(&b"application/octet-stream"[..])
    );
    assert_eq!(request.header("content-length"), Some(&b"7"[..]));
    assert_eq!(request.header("x-private"), None);
    assert_eq!(request.header("keep-alive"), None);
    assert_eq!(
        request.header("host"),
        Some(target.addr.to_string().as_bytes())
    );
    assert!(fake.requests().iter().any(|path| path.ends_with("/body")));
    assert_eq!(last_seq(&state), 1);
}

#[tokio::test]
async fn resumes_without_a_gap_after_the_process_is_killed_and_restarted() {
    let fake = Fake::start(FakeOptions::default()).await;
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(&fake, &target_url, &state, &[]);

    let mut first = Cli::spawn(&scratch, &[], &args);
    first.wait_for("connected").await;
    for seq in 1..=3 {
        fake.push(CaptureSpec::new("POST", "/x").body(seq.to_string().as_bytes()));
    }
    first.wait_for("#3 POST /x -> 202").await;
    first.kill().await;

    for seq in 4..=6 {
        fake.push(CaptureSpec::new("POST", "/x").body(seq.to_string().as_bytes()));
    }
    let mut second = Cli::spawn(&scratch, &[], &args);
    second.wait_for("resuming after #3").await;
    second.wait_for("#6 POST /x -> 202").await;
    second.interrupt();
    assert_eq!(second.exit_code().await, Some(0));

    assert_eq!(bodies(&target), vec!["1", "2", "3", "4", "5", "6"]);
    assert_eq!(last_seq(&state), 6);
    assert!(fake.last_event_ids().contains(&Some("3".to_owned())));
}

#[tokio::test]
async fn reconnects_after_the_server_drops_the_stream_and_resumes_by_last_event_id() {
    let fake = Fake::start(FakeOptions::default()).await;
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(&fake, &target_url, &state, &[]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("connected").await;

    fake.push(CaptureSpec::new("POST", "/x").body(b"1"));
    cli.wait_for("#1 POST /x -> 202").await;
    fake.drop_streams();
    fake.push(CaptureSpec::new("POST", "/x").body(b"2"));
    fake.push(CaptureSpec::new("POST", "/x").body(b"3"));
    cli.wait_for("#3 POST /x -> 202").await;

    assert_eq!(bodies(&target), vec!["1", "2", "3"]);
    assert!(fake.last_event_ids().contains(&Some("1".to_owned())));
}

#[tokio::test]
async fn replays_the_whole_history_oldest_first_across_pages_with_from_all() {
    let fake = Fake::start(FakeOptions {
        page_size: Some(2),
        ..FakeOptions::default()
    })
    .await;
    for seq in 1..=5 {
        fake.push(CaptureSpec::new("PUT", "/h").body(seq.to_string().as_bytes()));
    }
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(&fake, &target_url, &state, &["--from", "all"]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("#5 PUT /h -> 202").await;

    assert_eq!(bodies(&target), vec!["1", "2", "3", "4", "5"]);
}

#[tokio::test]
async fn sends_the_bearer_token_on_every_request() {
    let fake = Fake::start(FakeOptions {
        token: Some(TOKEN),
        ..FakeOptions::default()
    })
    .await;
    fake.push(CaptureSpec::new("POST", "/x").body(b"1"));
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(&fake, &target_url, &state, &["--from", "all"]);
    let mut cli = Cli::spawn(&scratch, &[("WEBHOOKS_LIVE_TOKEN", TOKEN)], &args);
    cli.wait_for("#1 POST /x -> 202").await;

    let expected = Some(format!("Bearer {TOKEN}"));
    let seen = fake.authorizations();
    assert!(seen.len() >= 4, "{seen:?}");
    assert!(seen.iter().all(|value| *value == expected), "{seen:?}");
    assert!(target.received()[0].header("authorization").is_none());
}

#[tokio::test]
async fn exits_with_the_auth_code_when_the_token_is_refused_without_printing_it() {
    let fake = Fake::start(FakeOptions {
        token: Some(TOKEN),
        ..FakeOptions::default()
    })
    .await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let args = forward_args(&fake, "http://127.0.0.1:9", &state, &[]);
    let mut cli = Cli::spawn(
        &scratch,
        &[("WEBHOOKS_LIVE_TOKEN", "whl_wrong_secret")],
        &args,
    );
    assert_eq!(cli.exit_code().await, Some(3));
    cli.wait_for("invalid_token").await;
    assert!(
        cli.transcript
            .iter()
            .all(|line| !line.contains("whl_wrong_secret")),
        "{:?}",
        cli.transcript
    );
}

#[tokio::test]
async fn exits_with_the_gone_code_when_the_endpoint_is_gone() {
    let fake = Fake::start(FakeOptions::default()).await;
    fake.set_gone();
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let args = forward_args(&fake, "http://127.0.0.1:9", &state, &[]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    assert_eq!(cli.exit_code().await, Some(4));
}

#[tokio::test]
async fn dry_run_sends_nothing_and_leaves_the_state_file_alone() {
    let fake = Fake::start(FakeOptions::default()).await;
    fake.push(CaptureSpec::new("POST", "/x").body(b"payload"));
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("/hooks");
    let args = forward_args(&fake, &target_url, &state, &["--from", "all", "--dry-run"]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    let line = cli.wait_for("dry run: would send POST").await;
    assert!(line.contains("/hooks/x"), "{line}");
    cli.interrupt();
    assert_eq!(cli.exit_code().await, Some(0));

    assert!(target.received().is_empty());
    assert!(!state.exists());
    assert!(!fake.requests().iter().any(|path| path.ends_with("/body")));
}

#[tokio::test]
async fn forwards_only_captures_that_pass_the_method_and_path_filters() {
    let fake = Fake::start(FakeOptions::default()).await;
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(
        &fake,
        &target_url,
        &state,
        &["--method", "post", "--path", "/stripe"],
    );
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("connected").await;

    fake.push(CaptureSpec::new("GET", "/stripe/a"));
    fake.push(CaptureSpec::new("POST", "/other"));
    fake.push(CaptureSpec::new("POST", "/stripe/b").body(b"yes"));
    cli.wait_for("#3 POST /stripe/b -> 202").await;

    assert_eq!(bodies(&target), vec!["yes"]);
    assert_eq!(last_seq(&state), 3);
}

#[tokio::test]
async fn rewrites_headers_with_set_remove_and_keep_host() {
    let fake = Fake::start(FakeOptions::default()).await;
    fake.push(
        CaptureSpec::new("POST", "/x")
            .header("Host", "webhooks.live")
            .header("X-Signature", "sig")
            .header("X-Drop", "1"),
    );
    let target = Target::start().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = target.url("");
    let args = forward_args(
        &fake,
        &target_url,
        &state,
        &[
            "--from",
            "all",
            "--keep-host",
            "-H",
            "X-Env: local",
            "--remove-header",
            "x-drop",
        ],
    );
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("#1 POST /x -> 202").await;

    let request = &target.received()[0];
    assert_eq!(request.header("host"), Some(&b"webhooks.live"[..]));
    assert_eq!(request.header("x-env"), Some(&b"local"[..]));
    assert_eq!(request.header("x-drop"), None);
    assert_eq!(request.header("x-signature"), Some(&b"sig"[..]));
}

#[tokio::test]
async fn keeps_running_and_counts_a_capture_handled_when_the_local_target_is_down() {
    let fake = Fake::start(FakeOptions::default()).await;
    let dead = closed_port().await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let target_url = format!("http://{dead}");
    let args = forward_args(&fake, &target_url, &state, &[]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("connected").await;

    fake.push(CaptureSpec::new("POST", "/x"));
    fake.push(CaptureSpec::new("POST", "/y"));
    cli.wait_for("#1 POST /x -> failed").await;
    cli.wait_for("#2 POST /y -> failed").await;

    assert!(cli.is_running());
    assert_eq!(last_seq(&state), 2);
}

#[tokio::test]
async fn stops_cleanly_with_exit_code_zero_on_interrupt() {
    let fake = Fake::start(FakeOptions::default()).await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let args = forward_args(&fake, "http://127.0.0.1:9", &state, &[]);
    let mut cli = Cli::spawn(&scratch, &[], &args);
    cli.wait_for("connected").await;
    eventually(|| state.exists()).await;
    cli.interrupt();
    assert_eq!(cli.exit_code().await, Some(0));
}

#[tokio::test]
async fn refuses_a_second_forwarder_on_the_same_state_file_with_the_state_code() {
    let fake = Fake::start(FakeOptions::default()).await;
    let scratch = Scratch::new();
    let state = scratch.state_file();
    let args = forward_args(&fake, "http://127.0.0.1:9", &state, &[]);
    let mut first = Cli::spawn(&scratch, &[], &args);
    first.wait_for("connected").await;
    let mut second = Cli::spawn(&scratch, &[], &args);
    assert_eq!(second.exit_code().await, Some(5));
    first.interrupt();
    assert_eq!(first.exit_code().await, Some(0));
}
