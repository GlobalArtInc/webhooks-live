mod common;

use common::{Client, Mail, SIMPLE, address, settings};
use serde_json::json;

#[tokio::test]
async fn the_banner_and_the_extension_list_name_only_what_is_supported() {
    let mail = Mail::start().await;
    let mut client = Client::connect(mail.addr).await;

    let banner = client.read_reply().await;
    let ehlo = client.say("EHLO client.example.org").await;

    assert_eq!(banner.code, 220);
    assert!(banner.text.contains("mail.test"));
    assert_eq!(ehlo.code, 250);
    for keyword in [
        "SIZE 10485760",
        "8BITMIME",
        "SMTPUTF8",
        "PIPELINING",
        "ENHANCEDSTATUSCODES",
    ] {
        assert!(ehlo.text.contains(keyword), "{keyword} in {}", ehlo.text);
    }
    for keyword in ["AUTH", "STARTTLS", "VRFY", "BDAT", "CHUNKING"] {
        assert!(!ehlo.text.contains(keyword), "{keyword} in {}", ehlo.text);
    }
}

#[tokio::test]
async fn a_message_becomes_an_email_capture_and_is_acknowledged_with_its_id() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;

    let reply = client.deliver(&address(&id), SIMPLE).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let stored = mail.captures(&id).await;
    assert_eq!(stored.len(), 1);
    let capture = &stored[0];
    assert!(reply.text.contains(&capture.id), "{reply:?}");
    assert!(reply.text.contains("2.0.0"));
    assert_eq!(capture.kind, "email");
    assert_eq!(capture.method, "SMTP");
    assert_eq!(capture.url, format!("mailto:{}", address(&id)));
    assert_eq!(capture.path, "");
    assert_eq!(capture.host, "mail.test");
    assert_eq!(capture.client_ip, "127.0.0.1");
    assert_eq!(capture.response_status, 250);
    assert_eq!(capture.body, format!("{SIMPLE}\r\n").into_bytes());
    assert_eq!(capture.meta["envelope_from"], "sender@example.org");
    assert_eq!(capture.meta["envelope_to"], json!([address(&id)]));
    assert_eq!(capture.meta["helo"], "client.example.org");
    assert_eq!(capture.meta["tls"], false);
    assert_eq!(capture.meta["subject"], "Hello there");
    assert_eq!(capture.meta["from"], "Alice <alice@example.org>");
    assert_eq!(capture.meta["to"], json!(["bob@example.org"]));
    assert_eq!(capture.meta["message_id"], "abc123@example.org");
    assert_eq!(capture.meta["parts"][0]["content_type"], "text/plain");
    assert!(capture.headers.to_string().contains("Hello there"));
}

#[tokio::test]
async fn dot_stuffing_is_removed_and_a_bare_lf_dot_does_not_end_the_message() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let content =
        "Subject: dots\r\n\r\n..stuffed\r\nbare\n.\nMAIL FROM:<smuggled@example.org>\r\nend";

    let reply = client.deliver(&address(&id), content).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let stored = mail.captures(&id).await;
    assert_eq!(stored.len(), 1);
    let body = String::from_utf8(stored[0].body.clone()).expect("text");
    assert!(body.contains("\r\n.stuffed\r\n"), "{body:?}");
    assert!(
        body.contains("bare\n.\nMAIL FROM:<smuggled@example.org>"),
        "{body:?}"
    );
}

#[tokio::test]
async fn every_unusable_recipient_gets_the_same_550() {
    let mail = Mail::start().await;
    let disabled = mail.endpoint(false).await;
    let unknown = whl_core::endpoint_id::EndpointId::generate().expect("entropy");
    let live = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let cases = [
        address(&disabled),
        address(&unknown),
        format!("{}@other.example", live.unpack()),
        format!("{}@mail.test.evil.example", live.unpack()),
        "postmaster@mail.test".to_owned(),
        format!("{}+bad=tag@mail.test", live.unpack()),
    ];

    for recipient in cases {
        let sender = client.say("MAIL FROM:<sender@example.org>").await;
        assert_eq!(sender.code, 250);
        let reply = client.say(&format!("RCPT TO:<{recipient}>")).await;
        assert_eq!(reply.text, "550 5.1.1 Recipient not found", "{recipient}");
        assert_eq!(client.say("RSET").await.code, 250);
    }
}

#[tokio::test]
async fn the_id_and_the_domain_are_case_insensitive_and_the_tag_is_kept() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let recipient = format!("{}+Signup-1@MAIL.Test", id.unpack().to_uppercase());

    let reply = client.deliver(&recipient, SIMPLE).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let stored = mail.captures(&id).await;
    assert_eq!(stored[0].path, "+signup-1");
    assert_eq!(stored[0].meta["tag"], "signup-1");
    assert_eq!(
        stored[0].url,
        format!("mailto:{}+signup-1@mail.test", id.unpack())
    );
}

#[tokio::test]
async fn commands_out_of_sequence_get_503() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::connect(mail.addr).await;
    client.read_reply().await;

    assert_eq!(client.say("MAIL FROM:<a@example.org>").await.code, 503);
    assert_eq!(client.say("EHLO client.example.org").await.code, 250);
    assert_eq!(
        client
            .say(&format!("RCPT TO:<{}>", address(&id)))
            .await
            .code,
        503
    );
    assert_eq!(client.say("DATA").await.code, 503);
    assert_eq!(client.say("MAIL FROM:<a@example.org>").await.code, 250);
    assert_eq!(client.say("MAIL FROM:<b@example.org>").await.code, 503);
    assert_eq!(
        client.say("DATA").await.text,
        "554 5.5.1 No valid recipients"
    );
}

#[tokio::test]
async fn unknown_bad_and_unsupported_commands_get_their_own_codes() {
    let mail = Mail::start().await;
    let mut client = Client::greeted(mail.addr).await;

    assert_eq!(client.say("FROB").await.code, 500);
    assert_eq!(client.say("MAIL nonsense").await.code, 501);
    assert_eq!(
        client
            .say("MAIL FROM:<a@example.org> BODY=BINARYMIME")
            .await
            .code,
        504
    );
    for command in [
        "AUTH PLAIN",
        "VRFY someone",
        "EXPN list",
        "BDAT 10 LAST",
        "HELP",
    ] {
        assert_eq!(client.say(command).await.code, 502, "{command}");
    }
    assert_eq!(client.say("STARTTLS").await.code, 502);
}

#[tokio::test]
async fn rset_noop_and_quit_answer_their_codes() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;

    assert_eq!(client.say("NOOP").await.code, 250);
    assert_eq!(client.envelope(&address(&id)).await.code, 250);
    assert_eq!(client.say("RSET").await.code, 250);
    assert_eq!(client.say("DATA").await.code, 503);
    let quit = client.say("QUIT").await;
    assert_eq!(quit.code, 221);
    client.expect_closed().await;
}

#[tokio::test]
async fn a_declared_size_over_the_limit_is_refused_before_the_data() {
    let mut config = settings();
    config.limits.max_message_bytes = 4096;
    let mail = Mail::start_with(config).await;
    let mut client = Client::greeted(mail.addr).await;

    let reply = client.say("MAIL FROM:<a@example.org> SIZE=4097").await;

    assert_eq!(reply.text, "552 5.3.4 Message size exceeds the limit");
    assert_eq!(
        client.say("MAIL FROM:<a@example.org> SIZE=4096").await.code,
        250
    );
}

#[tokio::test]
async fn the_null_sender_is_accepted_and_recorded_as_empty() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;

    assert_eq!(client.say("MAIL FROM:<>").await.code, 250);
    assert_eq!(
        client
            .say(&format!("RCPT TO:<{}>", address(&id)))
            .await
            .code,
        250
    );
    assert_eq!(client.data(SIMPLE).await.code, 250);

    assert_eq!(mail.captures(&id).await[0].meta["envelope_from"], "");
}

#[tokio::test]
async fn a_sender_outside_the_allow_list_is_refused_like_an_unknown_recipient() {
    let mail = Mail::start().await;
    let denied = mail.endpoint(true).await;
    mail.allow_only(&denied, "203.0.113.0/24").await;
    let allowed = mail.endpoint(true).await;
    mail.allow_only(&allowed, "127.0.0.0/8").await;
    let mut client = Client::greeted(mail.addr).await;

    let refused = client.envelope(&address(&denied)).await;
    assert_eq!(client.say("RSET").await.code, 250);
    let accepted = client.envelope(&address(&allowed)).await;

    assert_eq!(refused.text, "550 5.1.1 Recipient not found");
    assert_eq!(accepted.code, 250);
}

#[tokio::test]
async fn an_endpoint_over_its_hourly_quota_answers_451() {
    let mut config = settings();
    config.limits.messages_per_hour_per_endpoint = 2;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;

    for _ in 0..2 {
        assert_eq!(client.deliver(&address(&id), SIMPLE).await.code, 250);
    }
    let third = client.envelope(&address(&id)).await;

    assert_eq!(third.text, "451 4.7.1 Rate limit exceeded, try again later");
    assert_eq!(mail.capture_count(&id).await, 2);
}

#[tokio::test]
async fn a_full_endpoint_answers_552() {
    let mut config = settings();
    config.capture.max_captures = 1;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    assert_eq!(client.deliver(&address(&id), SIMPLE).await.code, 250);

    let refused = client.envelope(&address(&id)).await;

    assert_eq!(refused.text, "552 5.2.2 Mailbox full");
}

#[tokio::test]
async fn recipients_on_two_endpoints_make_one_capture_each_without_leaking_the_other_address() {
    let mail = Mail::start().await;
    let first = mail.endpoint(true).await;
    let second = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    assert_eq!(client.envelope(&address(&first)).await.code, 250);
    assert_eq!(
        client
            .say(&format!("RCPT TO:<{}>", address(&second)))
            .await
            .code,
        250
    );
    assert_eq!(
        client
            .say(&format!("RCPT TO:<{}+again@mail.test>", first.unpack()))
            .await
            .code,
        250
    );

    let reply = client.data(SIMPLE).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let first_mail = mail.captures(&first).await;
    let second_mail = mail.captures(&second).await;
    assert_eq!(first_mail.len(), 1);
    assert_eq!(second_mail.len(), 1);
    assert_eq!(
        second_mail[0].meta["envelope_to"],
        json!([address(&second)])
    );
    assert_eq!(
        first_mail[0].meta["envelope_to"],
        json!([
            address(&first),
            format!("{}+again@mail.test", first.unpack())
        ])
    );
}

#[tokio::test]
async fn smuggling_variants_of_the_terminator_do_not_end_the_message() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let content =
        "Subject: smuggle\r\n\r\na\r.\rb\r\nc\n.\r\nMAIL FROM:<smuggled@example.org>\r\nend";

    let reply = client.deliver(&address(&id), content).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let stored = mail.captures(&id).await;
    assert_eq!(stored.len(), 1);
    let body = String::from_utf8(stored[0].body.clone()).expect("text");
    assert!(
        body.contains("a\r.\rb\r\nc\n.\r\nMAIL FROM:<smuggled@example.org>"),
        "{body:?}"
    );
}

#[tokio::test]
async fn a_message_that_cannot_be_parsed_is_still_stored_with_its_reason() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let content = "X-Filler: value\r\n".repeat(80_000);

    let reply = client.deliver(&address(&id), &content).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    let stored = mail.captures(&id).await;
    assert_eq!(stored[0].meta["parse_error"], "header_block_too_large");
    assert_eq!(stored[0].meta["parts"], json!([]));
    assert_eq!(stored[0].body.len(), content.len() + 2);
}
