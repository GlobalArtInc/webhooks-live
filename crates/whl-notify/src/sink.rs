use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use url::Url;

const SILENT_HOLD: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkMode {
    Accept,
    RefuseRecipients,
    DeferRecipients,
    Silent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedMail {
    pub sender: String,
    pub recipients: Vec<String>,
    pub data: String,
}

impl CapturedMail {
    pub fn headers(&self) -> Vec<MailHeader> {
        let head = self.data.split("\r\n\r\n").next().unwrap_or_default();
        let mut headers: Vec<MailHeader> = Vec::new();
        for line in head.split("\r\n") {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(last) = headers.last_mut() {
                    last.value.push(' ');
                    last.value.push_str(line.trim());
                }
            } else if let Some((name, value)) = line.split_once(':') {
                headers.push(MailHeader {
                    name: name.trim().to_ascii_lowercase(),
                    value: value.trim().to_owned(),
                });
            }
        }
        headers
    }

    pub fn header(&self, name: &str) -> Option<String> {
        let wanted = name.to_ascii_lowercase();
        self.headers()
            .into_iter()
            .find(|header| header.name == wanted)
            .map(|header| header.value)
    }

    pub fn text(&self) -> String {
        let encoded: String = self.body().split_whitespace().collect();
        match STANDARD.decode(encoded) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
            Err(_) => self.body(),
        }
    }

    pub fn body(&self) -> String {
        self.data
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.to_owned())
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct SmtpSink {
    addr: SocketAddr,
    mails: Arc<Mutex<Vec<CapturedMail>>>,
    commands: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl SmtpSink {
    pub async fn start(mode: SinkMode) -> Self {
        Self::bind("127.0.0.1:0", mode)
            .await
            .expect("the sink binds")
    }

    pub async fn start_ipv6(mode: SinkMode) -> Option<Self> {
        Self::bind("[::1]:0", mode).await
    }

    async fn bind(address: &str, mode: SinkMode) -> Option<Self> {
        let listener = TcpListener::bind(address).await.ok()?;
        let addr = listener.local_addr().expect("the address is known");
        let mails = Arc::new(Mutex::new(Vec::new()));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let mails = Arc::clone(&mails);
            let commands = Arc::clone(&commands);
            async move {
                while let Ok((stream, _)) = listener.accept().await {
                    tokio::spawn(serve(
                        stream,
                        mode,
                        Arc::clone(&mails),
                        Arc::clone(&commands),
                    ));
                }
            }
        });
        Some(Self {
            addr,
            mails,
            commands,
            task,
        })
    }

    pub fn url(&self) -> Url {
        Url::parse(&format!("smtp://{}?tls=none", self.addr)).expect("a valid sink URL")
    }

    pub fn mails(&self) -> Vec<CapturedMail> {
        self.mails.lock().expect("the mail log is readable").clone()
    }

    pub fn commands(&self) -> Vec<String> {
        self.commands
            .lock()
            .expect("the command log is readable")
            .clone()
    }
}

impl Drop for SmtpSink {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(
    stream: TcpStream,
    mode: SinkMode,
    mails: Arc<Mutex<Vec<CapturedMail>>>,
    commands: Arc<Mutex<Vec<String>>>,
) {
    if mode == SinkMode::Silent {
        tokio::time::sleep(SILENT_HOLD).await;
        return;
    }
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read);
    if write.write_all(b"220 sink ready\r\n").await.is_err() {
        return;
    }
    let mut sender = String::new();
    let mut recipients: Vec<String> = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = line.trim_end().to_owned();
        let verb = command
            .split([' ', ':'])
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        commands
            .lock()
            .expect("the command log is writable")
            .push(verb.clone());
        let reply: &[u8] = match verb.as_str() {
            "EHLO" | "HELO" => b"250 sink\r\n",
            "MAIL" => {
                sender = bracketed(&command);
                recipients.clear();
                b"250 ok\r\n"
            }
            "RCPT" => match mode {
                SinkMode::RefuseRecipients => b"550 no such user\r\n",
                SinkMode::DeferRecipients => b"451 try again later\r\n",
                _ => {
                    recipients.push(bracketed(&command));
                    b"250 ok\r\n"
                }
            },
            "DATA" => {
                if write.write_all(b"354 go ahead\r\n").await.is_err() {
                    return;
                }
                let Some(data) = read_data(&mut lines).await else {
                    return;
                };
                mails
                    .lock()
                    .expect("the mail log is writable")
                    .push(CapturedMail {
                        sender: sender.clone(),
                        recipients: recipients.clone(),
                        data,
                    });
                b"250 queued\r\n"
            }
            "RSET" | "NOOP" => b"250 ok\r\n",
            "QUIT" => {
                let _ = write.write_all(b"221 bye\r\n").await;
                return;
            }
            _ => b"502 not implemented\r\n",
        };
        if write.write_all(reply).await.is_err() {
            return;
        }
    }
}

fn bracketed(command: &str) -> String {
    let start = command.find('<').map_or(0, |index| index + 1);
    let end = command.rfind('>').unwrap_or(command.len());
    command[start..end.max(start)].to_owned()
}

async fn read_data(lines: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<String> {
    let mut data = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        match lines.read_line(&mut line).await {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        if line == ".\r\n" || line == ".\n" {
            return Some(data);
        }
        let unstuffed = line
            .strip_prefix('.')
            .filter(|_| line.starts_with(".."))
            .unwrap_or(&line);
        data.push_str(unstuffed);
    }
}
