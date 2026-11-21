mod support;

use serde_json::json;
use support::{ChannelSpec, DISCORD_URL, SLACK_URL, TELEGRAM_TOKEN, limits, rig, wait_until};
use whl_store::notify::ChannelKind;

#[tokio::test]
async fn slack_gets_an_escaped_text_on_the_stored_path() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(ChannelKind::Slack, SLACK_URL))
        .await;
    rig.capture("POST", "/<!channel>&x", b"{}").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = rig.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].target, "/services/T0/B0/slack-secret-part");
    let body = seen[0].json();
    let text = body["text"].as_str().expect("a text");
    assert!(text.contains("&lt;!channel&gt;&amp;x"), "{text}");
    assert!(!text.contains('<'));
    assert_eq!(body.as_object().expect("an object").len(), 1);
}

#[tokio::test]
async fn discord_disables_mentions_and_stays_within_the_limit() {
    let rig = rig().await;
    let mut spec = ChannelSpec::url(ChannelKind::Discord, DISCORD_URL);
    spec.template = Some("@everyone {{ request.path }}");
    rig.channel(spec).await;
    let long = format!("/{}", "a".repeat(3000));
    rig.capture("POST", &long, b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let body = rig.server.seen()[0].json();
    assert_eq!(body["allowed_mentions"], json!({ "parse": [] }));
    let content = body["content"].as_str().expect("a content");
    assert!(content.chars().count() <= 2000);
    assert!(content.starts_with("@\u{200b}everyone"));
}

#[tokio::test]
async fn telegram_posts_the_token_path_with_a_chat_id_and_no_parse_mode() {
    let rig = rig().await;
    rig.token_secret("BOT_TOKEN", TELEGRAM_TOKEN).await;
    let mut spec = ChannelSpec::url(ChannelKind::Telegram, "unused");
    spec.secret = None;
    spec.config = json!({ "token_secret": "BOT_TOKEN", "chat_id": "-100123" });
    rig.channel(spec).await;
    rig.capture("GET", "/ping", b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = rig.server.seen();
    assert_eq!(seen[0].target, format!("/bot{TELEGRAM_TOKEN}/sendMessage"));
    let body = seen[0].json();
    assert_eq!(body["chat_id"], "-100123");
    assert_eq!(body["disable_web_page_preview"], true);
    assert!(body.get("parse_mode").is_none());
}

#[tokio::test]
async fn a_webhook_gets_the_capture_summary_and_a_delivery_header() {
    let rig = rig().await;
    let url = format!("http://{}/hook", rig.server.addr);
    rig.channel(ChannelSpec::url(ChannelKind::Webhook, &url))
        .await;
    let capture = rig.capture("PUT", "/a", b"x").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = rig.server.seen();
    assert_eq!(seen[0].target, "/hook");
    assert!(seen[0].header("x-webhooks-live-delivery").is_some());
    assert_eq!(seen[0].header("x-webhooks-live-hop"), Some("1"));
    assert!(
        seen[0]
            .header("user-agent")
            .is_some_and(|agent| agent.starts_with("webhooks.live/"))
    );
    let body = seen[0].json();
    assert_eq!(body["endpoint_id"], rig.endpoint.unpack());
    assert_eq!(body["capture"]["id"], capture.to_string());
    assert_eq!(body["capture"]["method"], "PUT");
    assert!(body["message"].as_str().expect("a message").contains("/a"));
}

#[tokio::test]
async fn discord_markdown_in_captured_text_is_escaped() {
    let rig = rig().await;
    rig.channel(ChannelSpec::url(ChannelKind::Discord, DISCORD_URL))
        .await;
    rig.capture("POST", "/[x](https://evil.test)", b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let body = rig.server.seen()[0].json();
    let content = body["content"].as_str().expect("a content");
    assert!(
        content.contains("\\[x\\]\\(https://evil.test\\)"),
        "{content}"
    );
}

#[tokio::test]
async fn a_template_message_is_rendered_from_the_capture() {
    let rig = rig().await;
    let url = format!("http://{}/hook", rig.server.addr);
    let mut spec = ChannelSpec::url(ChannelKind::Webhook, &url);
    spec.template = Some("hit {{ request.method }} {{ request.path }}");
    rig.channel(spec).await;
    rig.capture("DELETE", "/z", b"").await;
    let running = rig.start(limits(30, 300, 60));
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(rig.server.seen()[0].json()["message"], "hit DELETE /z");
}
