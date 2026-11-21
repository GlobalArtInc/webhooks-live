mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{Client, Mail, SIMPLE, address, settings};
use rustls::RootCertStore;
use rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use whl_mail::TlsFiles;

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Material {
    dir: PathBuf,
    files: TlsFiles,
    connector: TlsConnector,
}

impl Drop for Material {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn material() -> Material {
    let certified =
        rcgen::generate_simple_self_signed(vec!["mail.test".to_owned()]).expect("a certificate");
    let dir = std::env::temp_dir().join(format!(
        "whl-mail-tls-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a directory");
    let cert = dir.join("cert.pem");
    let key = dir.join("key.pem");
    std::fs::write(&cert, certified.cert.pem()).expect("the certificate file");
    std::fs::write(&key, certified.signing_key.serialize_pem()).expect("the key file");
    let mut roots = RootCertStore::empty();
    roots
        .add(certified.cert.der().clone())
        .expect("a trust anchor");
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Material {
        dir,
        files: TlsFiles { cert, key },
        connector: TlsConnector::from(Arc::new(config)),
    }
}

async fn start(material: &Material) -> Mail {
    let mut config = settings();
    config.tls = Some(material.files.clone());
    Mail::start_with(config).await
}

async fn upgrade(client: Client, material: &Material) -> Client {
    let name = ServerName::try_from("mail.test").expect("a server name");
    let tls = material
        .connector
        .connect(name, client.into_stream())
        .await
        .expect("the handshake succeeds");
    Client::from_stream(Box::new(tls))
}

#[tokio::test]
async fn starttls_upgrades_the_session_and_the_capture_records_tls() {
    let material = material();
    let mail = start(&material).await;
    let id = mail.endpoint(true).await;
    let mut client = Client::connect(mail.addr).await;
    client.read_reply().await;
    let plain = client.say("EHLO client.example.org").await;
    assert!(plain.text.contains("STARTTLS"), "{}", plain.text);
    assert_eq!(client.say("STARTTLS").await.code, 220);

    let mut secure = upgrade(client, &material).await;
    assert_eq!(secure.say("MAIL FROM:<a@example.org>").await.code, 503);
    let ehlo = secure.say("EHLO client.example.org").await;
    assert!(!ehlo.text.contains("STARTTLS"), "{}", ehlo.text);
    assert_eq!(secure.say("STARTTLS").await.code, 503);
    let reply = secure.deliver(&address(&id), SIMPLE).await;

    assert_eq!(reply.code, 250, "{reply:?}");
    assert_eq!(mail.captures(&id).await[0].meta["tls"], true);
}

#[tokio::test]
async fn bytes_pipelined_behind_starttls_close_the_connection() {
    let material = material();
    let mail = start(&material).await;
    let mut client = Client::greeted(mail.addr).await;

    client.send_raw(b"STARTTLS\r\nNOOP\r\n").await;
    let reply = client.read_reply().await;

    assert_eq!(reply.code, 554);
    client.expect_closed().await;
}
