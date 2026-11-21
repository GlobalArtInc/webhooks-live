use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use rustls::ServerConfig;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;

use crate::error::TlsError;
use crate::settings::TlsFiles;

pub const RELOAD_INTERVAL: Duration = Duration::from_secs(60);

pub struct TlsState {
    files: TlsFiles,
    current: RwLock<TlsAcceptor>,
}

impl TlsState {
    pub async fn load(files: TlsFiles) -> Result<Arc<Self>, TlsError> {
        let acceptor = read_acceptor(files.clone()).await?;
        Ok(Arc::new(Self {
            files,
            current: RwLock::new(acceptor),
        }))
    }

    pub async fn reload(&self) -> Result<(), TlsError> {
        let acceptor = read_acceptor(self.files.clone()).await?;
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = acceptor;
        Ok(())
    }

    pub fn acceptor(&self) -> TlsAcceptor {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

async fn read_acceptor(files: TlsFiles) -> Result<TlsAcceptor, TlsError> {
    tokio::task::spawn_blocking(move || build_acceptor(&files)).await?
}

fn read_file(path: &Path) -> Result<Vec<u8>, TlsError> {
    std::fs::read(path).map_err(|source| TlsError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn build_acceptor(files: &TlsFiles) -> Result<TlsAcceptor, TlsError> {
    let cert_bytes = read_file(&files.cert)?;
    let key_bytes = read_file(&files.key)?;
    let certs = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| TlsError::Pem {
            path: files.cert.clone(),
            message: error.to_string(),
        })?;
    if certs.is_empty() {
        return Err(TlsError::NoCertificate {
            path: files.cert.clone(),
        });
    }
    let key = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|error| match error {
        rustls_pki_types::pem::Error::NoItemsFound => TlsError::NoKey {
            path: files.key.clone(),
        },
        other => TlsError::Pem {
            path: files.key.clone(),
            message: other.to_string(),
        },
    })?;
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
