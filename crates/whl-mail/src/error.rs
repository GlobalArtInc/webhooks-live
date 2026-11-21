use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("cannot read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} holds no certificate", path.display())]
    NoCertificate { path: PathBuf },
    #[error("{} holds no private key", path.display())]
    NoKey { path: PathBuf },
    #[error("cannot decode {}: {message}", path.display())]
    Pem { path: PathBuf, message: String },
    #[error("the certificate and key are not usable: {0}")]
    Config(#[from] rustls::Error),
    #[error("the reload task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
}

#[derive(Debug, thiserror::Error)]
pub enum MailSetupError {
    #[error("the mail domain is missing or invalid")]
    Domain,
    #[error("the mail limits are not usable")]
    Limits,
    #[error("the TLS certificate and key must be configured together")]
    TlsFiles,
    #[error("cannot load the TLS certificate and key: {0}")]
    Tls(#[from] TlsError),
    #[error("cannot bind the mail listener on {addr}: {source}")]
    Bind { addr: SocketAddr, source: io::Error },
}
