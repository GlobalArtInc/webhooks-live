use std::borrow::Cow;
use std::net::IpAddr;

use smtp_proto::{
    MAIL_BODY_7BIT, MAIL_BODY_8BITMIME, MAIL_RET_FULL, MAIL_RET_HDRS, MAIL_SMTPUTF8, Request,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::Instant;

use crate::deliver::{Delivered, Envelope, clean_text, deliver};
use crate::recipient::{Recipient, parse_recipient};
use crate::shared::{Shared, message_result, refused};
use crate::wire::{DataFailure, LineFailure, Wire};

const ACCEPTED_MAIL_FLAGS: u64 =
    MAIL_BODY_7BIT | MAIL_BODY_8BITMIME | MAIL_SMTPUTF8 | MAIL_RET_FULL | MAIL_RET_HDRS;
const MAX_HELO_CHARS: usize = 255;

const BYE: &str = "221 2.0.0 Bye\r\n";
const OK: &str = "250 2.0.0 OK\r\n";
const SENDER_OK: &str = "250 2.1.0 OK\r\n";
const RECIPIENT_OK: &str = "250 2.1.5 OK\r\n";
const START_DATA: &str = "354 End data with <CR><LF>.<CR><LF>\r\n";
const START_TLS: &str = "220 2.0.0 Ready to start TLS\r\n";
const BAD_SEQUENCE: &str = "503 5.5.1 Bad sequence of commands\r\n";
const NEED_GREETING: &str = "503 5.5.1 Send EHLO or HELO first\r\n";
const NESTED_MAIL: &str = "503 5.5.1 Nested MAIL command\r\n";
const TLS_ACTIVE: &str = "503 5.5.1 TLS is already active\r\n";
const TLS_UNAVAILABLE: &str = "502 5.5.1 STARTTLS is not available\r\n";
const NO_RECIPIENTS: &str = "554 5.5.1 No valid recipients\r\n";
const NOT_IMPLEMENTED: &str = "502 5.5.1 Command not implemented\r\n";
const UNKNOWN_COMMAND: &str = "500 5.5.1 Command not recognized\r\n";
const SYNTAX: &str = "501 5.5.4 Syntax error in parameters or arguments\r\n";
const BAD_ADDRESS: &str = "501 5.1.7 Bad address syntax\r\n";
const BAD_PARAMETER: &str = "504 5.5.4 Parameter not supported\r\n";
const LINE_TOO_LONG: &str = "500 5.5.2 Line too long\r\n";
const SIZE_EXCEEDED: &str = "552 5.3.4 Message size exceeds the limit\r\n";
const TOO_MANY_RECIPIENTS: &str = "452 4.5.3 Too many recipients\r\n";
const NO_SUCH_RECIPIENT: &str = "550 5.1.1 Recipient not found\r\n";
const MAILBOX_FULL: &str = "552 5.2.2 Mailbox full\r\n";
const RATE_LIMITED: &str = "451 4.7.1 Rate limit exceeded, try again later\r\n";
const TEMPORARY_FAILURE: &str = "451 4.3.0 Temporary failure, try again later\r\n";
const MEMORY_EXHAUSTED: &str = "451 4.3.1 Server busy, try again later\r\n";
const STORAGE_FULL: &str = "452 4.3.1 Insufficient system storage\r\n";
const TIMEOUT: &str = "421 4.4.2 Timeout, closing connection\r\n";
const SHUTTING_DOWN: &str = "421 4.3.2 Shutting down, try again later\r\n";
const TOO_MANY_COMMANDS: &str = "421 4.7.0 Too many commands, closing connection\r\n";
const TOO_MANY_ERRORS: &str = "421 4.7.0 Too many errors, closing connection\r\n";
const TOO_MANY_MESSAGES: &str = "421 4.7.0 Too many messages in this session\r\n";
const PROTOCOL_VIOLATION: &str = "554 5.5.0 Data sent before the server was ready\r\n";

#[derive(Debug)]
struct Transaction {
    sender: String,
    recipients: Vec<Recipient>,
}

#[derive(Debug)]
pub struct SessionState {
    commands: u32,
    errors: u32,
    messages: u32,
    deadline: Instant,
    helo: Option<String>,
    transaction: Option<Transaction>,
}

impl SessionState {
    pub fn new(deadline: Instant) -> Self {
        Self {
            commands: 0,
            errors: 0,
            messages: 0,
            deadline,
            helo: None,
            transaction: None,
        }
    }

    pub fn restart_after_tls(&mut self) {
        self.helo = None;
        self.transaction = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Finished,
    Upgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Continue,
    Close,
    Upgrade,
}

enum Recipients {
    Accepted(Recipient),
    NotFound,
    Full,
    OverQuota,
    Unavailable,
}

pub struct Session<'a, S> {
    shared: &'a Shared,
    wire: &'a mut Wire<S>,
    state: &'a mut SessionState,
    client: IpAddr,
    tls: bool,
}

pub async fn converse<S>(
    shared: &Shared,
    wire: &mut Wire<S>,
    state: &mut SessionState,
    client: IpAddr,
    tls: bool,
) -> Flow
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut session = Session {
        shared,
        wire,
        state,
        client,
        tls,
    };
    loop {
        match session.next_command().await {
            Step::Continue => {}
            Step::Close => return Flow::Finished,
            Step::Upgrade => return Flow::Upgrade,
        }
    }
}

impl<S> Session<'_, S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    async fn send(&mut self, text: &str) -> bool {
        match self
            .wire
            .write(text.as_bytes(), self.shared.limits.write_timeout)
            .await
        {
            Ok(()) => true,
            Err(error) => {
                tracing::debug!(%error, "a mail reply could not be written");
                false
            }
        }
    }

    async fn reply(&mut self, text: &str) -> Step {
        if self.send(text).await {
            Step::Continue
        } else {
            Step::Close
        }
    }

    async fn finish_with(&mut self, text: &str, reason: &'static str) -> Step {
        refused(reason);
        self.send(text).await;
        Step::Close
    }

    async fn reject(&mut self, text: &str) -> Step {
        self.state.errors += 1;
        if self.state.errors >= self.shared.limits.max_errors {
            return self.finish_with(TOO_MANY_ERRORS, "too_many_errors").await;
        }
        self.reply(text).await
    }

    fn command_deadline(&self) -> Instant {
        (Instant::now() + self.shared.limits.command_timeout).min(self.state.deadline)
    }

    fn data_deadline(&self) -> Instant {
        (Instant::now() + self.shared.limits.data_timeout).min(self.state.deadline)
    }

    async fn next_command(&mut self) -> Step {
        if self.state.commands >= self.shared.limits.max_commands {
            return self
                .finish_with(TOO_MANY_COMMANDS, "too_many_commands")
                .await;
        }
        let deadline = self.command_deadline();
        let line = match self
            .wire
            .read_line(self.shared.limits.max_line_bytes, deadline)
            .await
        {
            Ok(line) => line,
            Err(LineFailure::TooLong) => {
                return self.finish_with(LINE_TOO_LONG, "line_too_long").await;
            }
            Err(LineFailure::Timeout) => {
                return self.finish_with(TIMEOUT, "command_timeout").await;
            }
            Err(LineFailure::Stopped) => {
                self.send(SHUTTING_DOWN).await;
                return Step::Close;
            }
            Err(LineFailure::Closed) => return Step::Close,
            Err(LineFailure::Io(error)) => {
                tracing::debug!(%error, "a mail connection failed while reading");
                return Step::Close;
            }
        };
        self.state.commands += 1;
        match Request::parse(&mut line.iter()) {
            Ok(request) => self.dispatch(request).await,
            Err(error) => self.reject(smtp_error_reply(&error)).await,
        }
    }

    async fn dispatch(&mut self, request: Request<Cow<'_, str>>) -> Step {
        match request {
            Request::Ehlo { host } => self.greet(&host, true).await,
            Request::Helo { host } => self.greet(&host, false).await,
            Request::Mail { from } => self.mail(from).await,
            Request::Rcpt { to } => self.rcpt(&to.address).await,
            Request::Data => self.data().await,
            Request::Rset => {
                self.state.transaction = None;
                self.reply(OK).await
            }
            Request::Noop { .. } => self.reply(OK).await,
            Request::Quit => {
                self.send(BYE).await;
                Step::Close
            }
            Request::StartTls => self.start_tls().await,
            Request::Lhlo { .. } => self.reject(UNKNOWN_COMMAND).await,
            Request::Bdat { .. }
            | Request::Auth { .. }
            | Request::Vrfy { .. }
            | Request::Expn { .. }
            | Request::Help { .. }
            | Request::Etrn { .. }
            | Request::Atrn { .. }
            | Request::Burl { .. } => self.reject(NOT_IMPLEMENTED).await,
        }
    }

    fn ehlo_text(&self) -> String {
        let mut lines = vec![
            self.shared.domain.unpack().to_owned(),
            format!("SIZE {}", self.shared.limits.max_message_bytes),
            "8BITMIME".to_owned(),
            "SMTPUTF8".to_owned(),
            "PIPELINING".to_owned(),
        ];
        if self.shared.tls.is_some() && !self.tls {
            lines.push("STARTTLS".to_owned());
        }
        lines.push("ENHANCEDSTATUSCODES".to_owned());
        let last = lines.len() - 1;
        lines
            .iter()
            .enumerate()
            .map(|(position, line)| {
                let separator = if position == last { ' ' } else { '-' };
                format!("250{separator}{line}\r\n")
            })
            .collect()
    }

    async fn greet(&mut self, host: &str, extended: bool) -> Step {
        self.state.helo = Some(clean_text(host, MAX_HELO_CHARS));
        self.state.transaction = None;
        if extended {
            let text = self.ehlo_text();
            self.reply(&text).await
        } else {
            let text = format!("250 {}\r\n", self.shared.domain.unpack());
            self.reply(&text).await
        }
    }

    async fn mail(&mut self, from: smtp_proto::MailFrom<Cow<'_, str>>) -> Step {
        if self.state.helo.is_none() {
            return self.reject(NEED_GREETING).await;
        }
        if self.state.transaction.is_some() {
            return self.reject(NESTED_MAIL).await;
        }
        if from.flags & !ACCEPTED_MAIL_FLAGS != 0 {
            return self.reject(BAD_PARAMETER).await;
        }
        if from.size > self.shared.limits.max_message_bytes {
            message_result("too_large");
            return self.reject(SIZE_EXCEEDED).await;
        }
        if self.state.messages >= self.shared.limits.messages_per_session {
            return self
                .finish_with(TOO_MANY_MESSAGES, "too_many_messages")
                .await;
        }
        if self.shared.storage_is_full() {
            return self.reply(STORAGE_FULL).await;
        }
        if !self.shared.message_allowed(self.client) {
            message_result("rate_limited");
            return self.reply(RATE_LIMITED).await;
        }
        self.state.transaction = Some(Transaction {
            sender: from.address.into_owned(),
            recipients: Vec::new(),
        });
        self.reply(SENDER_OK).await
    }

    async fn resolve(&self, address: &str) -> Recipients {
        let Some(recipient) = parse_recipient(address, &self.shared.domain) else {
            return Recipients::NotFound;
        };
        if self.shared.unknown_endpoints.contains(&recipient.endpoint) {
            return Recipients::NotFound;
        }
        let target = match self
            .shared
            .store
            .mail_target(&recipient.endpoint, self.shared.policy)
            .await
        {
            Ok(Some(target)) => target,
            Ok(None) => {
                self.shared.unknown_endpoints.insert(&recipient.endpoint);
                return Recipients::NotFound;
            }
            Err(error) => {
                tracing::warn!(%error, "the recipient lookup failed");
                return Recipients::Unavailable;
            }
        };
        let client = self.client.to_canonical();
        if !target.allow_cidrs.is_empty()
            && !target.allow_cidrs.iter().any(|net| net.contains(&client))
        {
            return Recipients::NotFound;
        }
        if target.full {
            return Recipients::Full;
        }
        if !self.shared.quota.has_room(&recipient.endpoint) {
            return Recipients::OverQuota;
        }
        Recipients::Accepted(recipient)
    }

    async fn rcpt(&mut self, address: &str) -> Step {
        let Some(transaction) = &self.state.transaction else {
            return self.reject(BAD_SEQUENCE).await;
        };
        let accepted_so_far = transaction.recipients.len();
        if accepted_so_far >= self.shared.limits.max_recipients {
            return self.reject(TOO_MANY_RECIPIENTS).await;
        }
        match self.resolve(address).await {
            Recipients::Accepted(recipient) => {
                if let Some(transaction) = self.state.transaction.as_mut()
                    && !transaction.recipients.contains(&recipient)
                {
                    transaction.recipients.push(recipient);
                }
                self.reply(RECIPIENT_OK).await
            }
            Recipients::NotFound => {
                message_result("refused_recipient");
                self.reject(NO_SUCH_RECIPIENT).await
            }
            Recipients::Full => self.reply(MAILBOX_FULL).await,
            Recipients::OverQuota => {
                message_result("quota");
                self.reply(RATE_LIMITED).await
            }
            Recipients::Unavailable => {
                message_result("store_unavailable");
                self.reply(TEMPORARY_FAILURE).await
            }
        }
    }

    async fn data(&mut self) -> Step {
        let transaction = match self.state.transaction.take() {
            Some(transaction) if !transaction.recipients.is_empty() => transaction,
            other => {
                let reply = if other.is_some() {
                    NO_RECIPIENTS
                } else {
                    BAD_SEQUENCE
                };
                self.state.transaction = other;
                return self.reject(reply).await;
            }
        };
        if self.wire.buffered() > 0 {
            return self.finish_with(PROTOCOL_VIOLATION, "pipelined_data").await;
        }
        if !self.send(START_DATA).await {
            return Step::Close;
        }
        let deadline = self.data_deadline();
        let collected = match self
            .wire
            .read_data(
                self.shared.limits.max_message_bytes,
                deadline,
                &self.shared.memory,
            )
            .await
        {
            Ok(collected) => collected,
            Err(DataFailure::TooLarge) => {
                message_result("too_large");
                self.send(SIZE_EXCEEDED).await;
                return Step::Close;
            }
            Err(DataFailure::Timeout) => {
                return self.finish_with(TIMEOUT, "data_timeout").await;
            }
            Err(DataFailure::OutOfMemory) => {
                return self.finish_with(MEMORY_EXHAUSTED, "memory").await;
            }
            Err(DataFailure::Stopped) => {
                self.send(SHUTTING_DOWN).await;
                return Step::Close;
            }
            Err(DataFailure::Closed) => return Step::Close,
            Err(DataFailure::Io(error)) => {
                tracing::debug!(%error, "a mail connection failed during DATA");
                return Step::Close;
            }
        };
        let envelope = Envelope {
            sender: transaction.sender,
            recipients: transaction.recipients,
            helo: self.state.helo.clone(),
            tls: self.tls,
            client: self.client,
        };
        let outcome = deliver(self.shared, &envelope, collected).await;
        self.state.messages += 1;
        let reply = match outcome {
            Delivered::Stored(id) => format!("250 2.0.0 {id}\r\n"),
            Delivered::Full => MAILBOX_FULL.to_owned(),
            Delivered::Gone => NO_SUCH_RECIPIENT.to_owned(),
            Delivered::OverQuota => RATE_LIMITED.to_owned(),
            Delivered::Unavailable => TEMPORARY_FAILURE.to_owned(),
        };
        self.reply(&reply).await
    }

    async fn start_tls(&mut self) -> Step {
        if self.tls {
            return self.reject(TLS_ACTIVE).await;
        }
        if self.shared.tls.is_none() {
            return self.reject(TLS_UNAVAILABLE).await;
        }
        if self.state.helo.is_none() {
            return self.reject(NEED_GREETING).await;
        }
        if self.state.transaction.is_some() {
            return self.reject(BAD_SEQUENCE).await;
        }
        if self.wire.buffered() > 0 {
            return self
                .finish_with(PROTOCOL_VIOLATION, "pipelined_starttls")
                .await;
        }
        if self.send(START_TLS).await {
            Step::Upgrade
        } else {
            Step::Close
        }
    }
}

fn smtp_error_reply(error: &smtp_proto::Error) -> &'static str {
    use smtp_proto::Error;
    match error {
        Error::UnknownCommand => UNKNOWN_COMMAND,
        Error::InvalidSenderAddress | Error::InvalidRecipientAddress => BAD_ADDRESS,
        Error::InvalidParameter { .. } | Error::UnsupportedParameter { .. } => BAD_PARAMETER,
        Error::NeedsMoreData { .. }
        | Error::SyntaxError { .. }
        | Error::ResponseTooLong
        | Error::InvalidResponse { .. } => SYNTAX,
    }
}
