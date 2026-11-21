mod common;

use std::time::{Duration, Instant};

use common::{Client, Mail, SIMPLE, address, settings};

#[tokio::test]
async fn an_oversized_command_line_is_answered_500_and_the_connection_closes() {
    let mail = Mail::start().await;
    let mut client = Client::greeted(mail.addr).await;

    client.send_raw(&[b'A'; 100_000]).await;
    let reply = client.read_reply().await;

    assert_eq!(reply.text, "500 5.5.2 Line too long");
    client.expect_closed().await;
}

#[tokio::test]
async fn an_endless_data_stream_is_cut_at_the_size_limit() {
    let mut config = settings();
    config.limits.max_message_bytes = 16 * 1024;
    config.limits.memory_budget = 1024 * 1024;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    assert_eq!(client.envelope(&address(&id)).await.code, 250);
    assert_eq!(client.say("DATA").await.code, 354);

    for _ in 0..64 {
        client.send_raw(&[b'x'; 1024]).await;
    }
    let reply = client.read_reply().await;

    assert_eq!(reply.text, "552 5.3.4 Message size exceeds the limit");
    client.expect_closed().await;
    assert_eq!(mail.capture_count(&id).await, 0);
}

#[tokio::test]
async fn a_data_phase_that_goes_quiet_times_out() {
    let mut config = settings();
    config.limits.data_timeout = Duration::from_millis(300);
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    assert_eq!(client.envelope(&address(&id)).await.code, 250);
    assert_eq!(client.say("DATA").await.code, 354);
    client.send_raw(b"Subject: never finished\r\n").await;

    let reply = client.read_reply().await;

    assert_eq!(reply.code, 421);
    client.expect_closed().await;
    assert_eq!(mail.capture_count(&id).await, 0);
}

#[tokio::test]
async fn a_slow_drip_of_a_command_line_times_out_at_the_line_deadline() {
    let mut config = settings();
    config.limits.command_timeout = Duration::from_millis(1000);
    let mail = Mail::start_with(config).await;
    let mut client = Client::connect(mail.addr).await;
    client.read_reply().await;
    let started = Instant::now();

    for byte in b"EHL" {
        client.send_raw(&[*byte]).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    let reply = client.try_read_reply().await;

    assert_eq!(reply.map(|reply| reply.code), Some(421));
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
    client.expect_closed().await;
}

#[tokio::test]
async fn a_thousand_recipients_stop_at_ten_and_end_in_a_closed_session() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    assert_eq!(client.say("MAIL FROM:<sender@example.org>").await.code, 250);

    let mut accepted = 0;
    let mut closed = false;
    for number in 0..1000 {
        client
            .send_raw(format!("RCPT TO:<{}+n{number}@mail.test>\r\n", id.unpack()).as_bytes())
            .await;
        match client.try_read_reply().await {
            Some(reply) if reply.code == 250 => accepted += 1,
            Some(reply) if reply.code == 452 => {}
            Some(reply) => {
                assert_eq!(reply.code, 421, "{reply:?}");
                closed = true;
                break;
            }
            None => {
                closed = true;
                break;
            }
        }
    }

    assert_eq!(accepted, 10);
    assert!(closed);
}

#[tokio::test]
async fn a_flood_of_noops_ends_at_the_command_cap() {
    let mut config = settings();
    config.limits.max_commands = 50;
    let mail = Mail::start_with(config).await;
    let mut client = Client::greeted(mail.addr).await;

    let mut last = None;
    for _ in 0..60 {
        client.send_raw(b"NOOP\r\n").await;
        last = client.try_read_reply().await;
        if last.as_ref().is_none_or(|reply| reply.code == 421) {
            break;
        }
    }

    assert_eq!(
        last.map(|reply| reply.text),
        Some("421 4.7.0 Too many commands, closing connection".to_owned())
    );
    client.expect_closed().await;
}

#[tokio::test]
async fn repeated_bad_commands_end_the_session() {
    let mail = Mail::start().await;
    let mut client = Client::greeted(mail.addr).await;

    let mut last = None;
    for _ in 0..20 {
        client.send_raw(b"FROB\r\n").await;
        last = client.try_read_reply().await;
        if last.as_ref().is_none_or(|reply| reply.code == 421) {
            break;
        }
    }

    assert_eq!(last.map(|reply| reply.code), Some(421));
}

#[tokio::test]
async fn a_pipelined_burst_is_answered_in_order() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let burst = format!(
        "MAIL FROM:<a@example.org>\r\nRCPT TO:<{}>\r\nRCPT TO:<nobody@mail.test>\r\nDATA\r\n",
        address(&id)
    );

    client.send_raw(burst.as_bytes()).await;
    let codes = [
        client.read_reply().await.code,
        client.read_reply().await.code,
        client.read_reply().await.code,
        client.read_reply().await.code,
    ];

    assert_eq!(codes, [250, 250, 550, 354]);
}

#[tokio::test]
async fn bytes_sent_right_behind_data_are_a_protocol_violation() {
    let mail = Mail::start().await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    let burst = format!(
        "MAIL FROM:<a@example.org>\r\nRCPT TO:<{}>\r\nDATA\r\nSubject: early\r\n\r\nbody\r\n.\r\n",
        address(&id)
    );

    client.send_raw(burst.as_bytes()).await;
    let mut last = None;
    while let Some(reply) = client.try_read_reply().await {
        last = Some(reply);
    }

    assert_eq!(last.map(|reply| reply.code), Some(554));
    assert_eq!(mail.capture_count(&id).await, 0);
}

#[tokio::test]
async fn the_per_address_session_cap_refuses_the_next_connection() {
    let mut config = settings();
    config.limits.sessions_per_ip = 2;
    let mail = Mail::start_with(config).await;
    let mut first = Client::greeted(mail.addr).await;
    let mut second = Client::greeted(mail.addr).await;

    let mut third = Client::connect(mail.addr).await;
    let refused = third.read_reply().await;

    assert_eq!(refused.code, 421);
    third.expect_closed().await;
    assert_eq!(first.say("NOOP").await.code, 250);
    assert_eq!(second.say("NOOP").await.code, 250);
    drop(first);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut fourth = Client::connect(mail.addr).await;
    assert_eq!(fourth.read_reply().await.code, 220);
}

#[tokio::test]
async fn the_global_session_cap_refuses_the_next_connection() {
    let mut config = settings();
    config.limits.max_sessions = 1;
    let mail = Mail::start_with(config).await;
    let _held = Client::greeted(mail.addr).await;

    let mut second = Client::connect(mail.addr).await;
    let reply = second.try_read_reply().await;

    assert!(reply.is_none_or(|reply| reply.code == 421));
    second.expect_closed().await;
}

#[tokio::test]
async fn the_message_cap_per_session_ends_the_session() {
    let mut config = settings();
    config.limits.messages_per_session = 2;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    for _ in 0..2 {
        assert_eq!(client.deliver(&address(&id), SIMPLE).await.code, 250);
    }

    let reply = client.say("MAIL FROM:<a@example.org>").await;

    assert_eq!(reply.code, 421);
    client.expect_closed().await;
}

#[tokio::test]
async fn the_message_rate_per_client_address_answers_451() {
    let mut config = settings();
    config.limits.messages_per_minute_per_ip = 2;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::greeted(mail.addr).await;
    for _ in 0..2 {
        assert_eq!(client.deliver(&address(&id), SIMPLE).await.code, 250);
    }

    let reply = client.say("MAIL FROM:<a@example.org>").await;

    assert_eq!(reply.code, 451);
}

#[tokio::test]
async fn an_exhausted_memory_budget_refuses_the_data_phase_with_451() {
    let mut config = settings();
    config.limits.max_message_bytes = 100 * 1024;
    config.limits.memory_budget = 360 * 1024;
    let mail = Mail::start_with(config).await;
    let id = mail.endpoint(true).await;
    let mut holder = Client::greeted(mail.addr).await;
    assert_eq!(holder.envelope(&address(&id)).await.code, 250);
    assert_eq!(holder.say("DATA").await.code, 354);
    holder.send_raw(b"Subject: holding\r\n\r\n").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut other = Client::greeted(mail.addr).await;
    assert_eq!(other.envelope(&address(&id)).await.code, 250);
    assert_eq!(other.say("DATA").await.code, 354);
    other.send_raw(b"Subject: starved\r\n\r\n").await;

    let reply = other.read_reply().await;

    assert_eq!(reply.code, 451);
    other.expect_closed().await;
}

#[tokio::test]
async fn shutdown_closes_idle_sessions_with_421() {
    let mail = Mail::start().await;
    let mut client = Client::greeted(mail.addr).await;

    let stopping = tokio::spawn(mail.shutdown());
    let reply = client.try_read_reply().await;

    assert_eq!(reply.map(|reply| reply.code), Some(421));
    stopping.await.expect("shutdown completes");
}
