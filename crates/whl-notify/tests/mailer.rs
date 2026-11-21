use std::time::{Duration, Instant};

use whl_notify::{MailError, MailSettings, Mailer, OutgoingMail, SinkMode, SmtpSink};

const FROM: &str = "noreply@hooks.example.test";

fn mailer(sink: &SmtpSink) -> Mailer {
    Mailer::new(&MailSettings::new(sink.url(), FROM.to_owned())).expect("the mailer is valid")
}

fn mail(to: &str, subject: &str, body: &str) -> OutgoingMail {
    OutgoingMail {
        to: to.to_owned(),
        subject: subject.to_owned(),
        body: body.to_owned(),
        unsubscribe: Some(
            "https://hooks.example.test/api/v1/notifications/disable?token=t".to_owned(),
        ),
    }
}

#[tokio::test]
async fn a_subject_or_body_cannot_add_headers_or_smtp_commands() {
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let hostile_body = "first\r\n.\r\nQUIT\r\nMAIL FROM:<evil@example.test>\r\n.hidden\r\n";
    mailer(&sink)
        .send(&mail(
            "ann@mail.example.test",
            "hi\r\nBcc: evil@example.test",
            hostile_body,
        ))
        .await
        .expect("the message is accepted");

    let mails = sink.mails();
    assert_eq!(mails.len(), 1);
    let sent = &mails[0];
    assert_eq!(sent.recipients, vec!["ann@mail.example.test".to_owned()]);
    assert!(sent.header("bcc").is_none());
    assert_eq!(
        sent.header("subject").as_deref(),
        Some("hi Bcc: evil@example.test")
    );
    assert_eq!(
        sent.text().replace("\r\n", "\n"),
        hostile_body.replace("\r\n", "\n")
    );
    let recipients = sink
        .commands()
        .iter()
        .filter(|verb| *verb == "RCPT")
        .count();
    let senders = sink
        .commands()
        .iter()
        .filter(|verb| *verb == "MAIL")
        .count();
    assert_eq!((recipients, senders), (1, 1));
}

#[tokio::test]
async fn a_recipient_with_line_breaks_or_a_second_address_is_refused_before_connecting() {
    let sink = SmtpSink::start(SinkMode::Accept).await;
    let mailer = mailer(&sink);
    for hostile in [
        "ann@mail.example.test\r\nBcc: evil@example.test",
        "ann@mail.example.test,evil@example.test",
        "<ann@mail.example.test>",
    ] {
        let outcome = mailer.send(&mail(hostile, "hello", "body")).await;
        assert_eq!(outcome, Err(MailError::Recipient), "{hostile:?}");
    }
    assert!(sink.commands().is_empty());
}

#[tokio::test]
async fn a_server_that_never_answers_is_given_up_on_within_the_bounds() {
    let sink = SmtpSink::start(SinkMode::Silent).await;
    let mut settings = MailSettings::new(sink.url(), FROM.to_owned());
    settings.command_timeout = Duration::from_millis(300);
    settings.deadline = Duration::from_secs(2);
    let mailer = Mailer::new(&settings).expect("the mailer is valid");

    let started = Instant::now();
    let outcome = mailer
        .send(&mail("ann@mail.example.test", "hello", "body"))
        .await;

    assert!(started.elapsed() < Duration::from_secs(5));
    let error = outcome.expect_err("the send fails");
    assert!(error.is_retryable(), "{error:?}");
}

#[tokio::test]
async fn an_ipv6_loopback_server_is_reached_without_brackets() {
    let Some(sink) = SmtpSink::start_ipv6(SinkMode::Accept).await else {
        return;
    };

    mailer(&sink)
        .send(&mail("ann@mail.example.test", "hello", "body"))
        .await
        .expect("the message is accepted over IPv6");

    assert_eq!(sink.mails().len(), 1);
}
