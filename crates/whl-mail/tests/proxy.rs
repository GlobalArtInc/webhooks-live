mod common;

use std::time::Duration;

use common::{Client, Mail, SIMPLE, address, settings};
use whl_core::proxy_protocol::SIGNATURE;

fn header(command: u8, source: [u8; 4], source_port: u16) -> Vec<u8> {
    let mut bytes = SIGNATURE.to_vec();
    bytes.push(0x20 | command);
    bytes.push(0x11);
    bytes.extend_from_slice(&12_u16.to_be_bytes());
    bytes.extend_from_slice(&source);
    bytes.extend_from_slice(&[10, 0, 0, 5]);
    bytes.extend_from_slice(&source_port.to_be_bytes());
    bytes.extend_from_slice(&25_u16.to_be_bytes());
    bytes
}

fn behind_proxy() -> whl_mail::MailSettings {
    let mut config = settings();
    config.trusted_proxies = vec!["127.0.0.0/8".parse().expect("network")];
    config
}

async fn with_header(mail: &Mail, bytes: &[u8]) -> Client {
    let mut client = Client::connect(mail.addr).await;
    client.send_raw(bytes).await;
    client
}

#[tokio::test]
async fn a_trusted_proxy_header_sets_the_client_address() {
    let mail = Mail::start_with(behind_proxy()).await;
    let id = mail.endpoint(true).await;
    let mut client = with_header(&mail, &header(1, [203, 0, 113, 9], 51000)).await;
    assert_eq!(client.read_reply().await.code, 220);
    assert_eq!(client.say("EHLO client.example.org").await.code, 250);

    let reply = client.deliver(&address(&id), SIMPLE).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    assert_eq!(mail.captures(&id).await[0].client_ip, "203.0.113.9");
}

#[tokio::test]
async fn the_sender_allow_list_judges_the_address_from_the_header() {
    let mail = Mail::start_with(behind_proxy()).await;
    let id = mail.endpoint(true).await;
    mail.allow_only(&id, "203.0.113.0/24").await;

    let mut inside = with_header(&mail, &header(1, [203, 0, 113, 9], 1)).await;
    assert_eq!(inside.read_reply().await.code, 220);
    assert_eq!(inside.say("EHLO a.example.org").await.code, 250);
    let accepted = inside.envelope(&address(&id)).await;

    let mut outside = with_header(&mail, &header(1, [198, 51, 100, 1], 1)).await;
    assert_eq!(outside.read_reply().await.code, 220);
    assert_eq!(outside.say("EHLO b.example.org").await.code, 250);
    let refused = outside.envelope(&address(&id)).await;

    assert_eq!(accepted.code, 250);
    assert_eq!(refused.code, 550);
}

#[tokio::test]
async fn a_bad_header_closes_the_connection_without_a_banner() {
    let mail = Mail::start_with(behind_proxy()).await;
    let mut text = with_header(&mail, b"EHLO client.example.org\r\n").await;
    let mut version_one = with_header(&mail, b"PROXY TCP4 203.0.113.9 10.0.0.5 51000 25\r\n").await;
    let mut wrong_version = header(1, [203, 0, 113, 9], 1);
    wrong_version[12] = 0x11;
    let mut bad_version = with_header(&mail, &wrong_version).await;

    text.expect_closed().await;
    version_one.expect_closed().await;
    bad_version.expect_closed().await;
}

#[tokio::test]
async fn a_local_command_header_closes_the_connection() {
    let mail = Mail::start_with(behind_proxy()).await;
    let mut client = with_header(&mail, &header(0, [0, 0, 0, 0], 0)).await;

    client.expect_closed().await;
}

#[tokio::test]
async fn a_peer_outside_the_trusted_list_is_refused_even_with_a_valid_header() {
    let mut config = settings();
    config.trusted_proxies = vec!["10.0.0.0/8".parse().expect("network")];
    let mail = Mail::start_with(config).await;
    let mut client = with_header(&mail, &header(1, [203, 0, 113, 9], 1)).await;

    client.expect_closed().await;
}

#[tokio::test]
async fn a_header_that_never_completes_is_dropped_at_the_deadline() {
    let mut config = behind_proxy();
    config.limits.proxy_timeout = Duration::from_millis(300);
    let mail = Mail::start_with(config).await;
    let mut client = with_header(&mail, &SIGNATURE[..5]).await;

    client.expect_closed().await;
}
