#![allow(dead_code)]

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hickory_proto::op::{Edns, Message, MessageType, OpCode, Query};
use hickory_proto::rr::rdata::opt::{ClientSubnet, EdnsCode, EdnsOption};
use hickory_proto::rr::{DNSClass, Name, RecordType};
use sqlx::Row;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;
use whl_core::endpoint_id::EndpointId;
use whl_core::proxy_protocol::SIGNATURE;
use whl_core::response::ResponseConfig;
use whl_dns::{DnsHandle, DnsServer, DnsSettings, StorageProbe};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

pub const ZONE: &str = "dns.test.example";
pub const WAIT: Duration = Duration::from_millis(400);
const RETENTION: Duration = Duration::from_secs(7 * 86_400);

pub struct Harness {
    pub db: TestDb,
    pub store: Store,
    pub endpoint: EndpointId,
    pub udp: SocketAddr,
    pub tcp: SocketAddr,
    pub handle: DnsHandle,
    pub storage_full: Arc<AtomicBool>,
    stop: watch::Sender<bool>,
}

pub struct StoredRow {
    pub kind: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub host: String,
    pub client_ip: String,
    pub response_status: i16,
    pub meta: serde_json::Value,
    pub id: String,
}

pub fn base_settings() -> DnsSettings {
    DnsSettings::new(
        "127.0.0.1:0".parse().expect("literal address"),
        ZONE,
        "ns.test.example",
        "hostmaster@test.example",
    )
    .expect("the test settings are valid")
}

pub async fn endpoint_in(store: &Store) -> EndpointId {
    let record = store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                cors: false,
                overflow: Overflow::Refuse,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    store
        .set_dns_enabled(&record.id, true)
        .await
        .expect("the flag is written");
    record.id
}

pub async fn start() -> Harness {
    start_with(|_| {}).await
}

pub async fn start_with(adjust: impl FnOnce(&mut DnsSettings)) -> Harness {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let endpoint = endpoint_in(&store).await;
    let mut settings = base_settings();
    settings.answer_a = Some(Ipv4Addr::new(192, 0, 2, 7));
    settings.answer_aaaa = Some("2001:db8::7".parse().expect("literal address"));
    adjust(&mut settings);
    let server = DnsServer::bind(settings).await.expect("sockets bind");
    let (udp, tcp) = (server.udp_addr(), server.tcp_addr());
    let (stop, stopped) = watch::channel(false);
    let storage_full = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&storage_full);
    let probe = StorageProbe::new(move || flag.load(Ordering::Relaxed));
    let handle = server
        .serve(store.clone(), probe, stopped)
        .expect("the responder starts");
    Harness {
        db,
        store,
        endpoint,
        udp,
        tcp,
        handle,
        storage_full,
        stop,
    }
}

impl Harness {
    pub fn name(&self) -> String {
        format!("{}.{ZONE}", self.endpoint)
    }

    pub async fn rows(&self) -> Vec<StoredRow> {
        let rows = sqlx::query(
            "SELECT kind, method, url, path, host, client_ip::text AS client_ip, \
             response_status, meta::text AS meta, id::text AS id \
             FROM captures WHERE endpoint_id = $1 ORDER BY seq",
        )
        .bind(self.endpoint.unpack())
        .fetch_all(self.db.pool())
        .await
        .expect("captures are readable");
        rows.iter()
            .map(|row| StoredRow {
                kind: row.get("kind"),
                method: row.get("method"),
                url: row.get("url"),
                path: row.get("path"),
                host: row.get("host"),
                client_ip: row.get("client_ip"),
                response_status: row.get("response_status"),
                meta: serde_json::from_str(&row.get::<String, _>("meta")).expect("meta is JSON"),
                id: row.get("id"),
            })
            .collect()
    }

    pub async fn stop(self) {
        let _ = self.stop.send(true);
        self.handle.join().await;
    }
}

pub fn query_message(name: &str, record_type: RecordType) -> Message {
    let mut message = Message::new(0x1234, MessageType::Query, OpCode::Query);
    message.metadata.recursion_desired = true;
    message.add_query(Query::query(
        Name::from_ascii(name).expect("valid test name"),
        record_type,
    ));
    message
}

pub fn query(name: &str, record_type: RecordType) -> Vec<u8> {
    query_message(name, record_type)
        .to_vec()
        .expect("the query encodes")
}

pub fn query_with_edns(name: &str, record_type: RecordType, payload: u16) -> Vec<u8> {
    let mut message = query_message(name, record_type);
    let mut edns = Edns::new();
    edns.set_max_payload(payload);
    message.set_edns(edns);
    message.to_vec().expect("the query encodes")
}

pub fn query_with_subnet(name: &str, record_type: RecordType) -> Vec<u8> {
    let mut message = query_message(name, record_type);
    let mut edns = Edns::new();
    edns.set_max_payload(1232);
    edns.options_mut().options.push((
        EdnsCode::Subnet,
        EdnsOption::Subnet(ClientSubnet::new(
            Ipv4Addr::new(198, 51, 100, 0).into(),
            24,
            0,
        )),
    ));
    message.set_edns(edns);
    message.to_vec().expect("the query encodes")
}

pub fn query_in_class(name: &str, class: DNSClass) -> Vec<u8> {
    let mut message = query_message(name, RecordType::A);
    message.queries[0].set_query_class(class);
    message.to_vec().expect("the query encodes")
}

pub fn decode(bytes: &[u8]) -> Message {
    Message::from_vec(bytes).expect("the response decodes")
}

pub async fn udp_exchange(server: SocketAddr, packet: &[u8]) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client binds");
    socket
        .send_to(packet, server)
        .await
        .expect("datagram is sent");
    receive(&socket).await
}

pub async fn receive(socket: &UdpSocket) -> Option<Vec<u8>> {
    let mut buffer = vec![0_u8; 4096];
    match tokio::time::timeout(WAIT, socket.recv_from(&mut buffer)).await {
        Ok(Ok((length, _))) => Some(buffer[..length].to_vec()),
        _ => None,
    }
}

pub fn framed(packet: &[u8]) -> Vec<u8> {
    let mut bytes = (u16::try_from(packet.len()).expect("small packet"))
        .to_be_bytes()
        .to_vec();
    bytes.extend_from_slice(packet);
    bytes
}

pub async fn read_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut prefix = [0_u8; 2];
    tokio::time::timeout(WAIT, stream.read_exact(&mut prefix))
        .await
        .ok()?
        .ok()?;
    let mut body = vec![0_u8; usize::from(u16::from_be_bytes(prefix))];
    tokio::time::timeout(WAIT, stream.read_exact(&mut body))
        .await
        .ok()?
        .ok()?;
    Some(body)
}

pub async fn tcp_exchange(server: SocketAddr, packet: &[u8]) -> Option<Vec<u8>> {
    let mut stream = TcpStream::connect(server).await.expect("client connects");
    stream
        .write_all(&framed(packet))
        .await
        .expect("frame is sent");
    read_frame(&mut stream).await
}

pub fn proxy_v2(source: SocketAddr, datagram: bool) -> Vec<u8> {
    let mut header = SIGNATURE.to_vec();
    header.push(0x21);
    let transport = if datagram { 0x2 } else { 0x1 };
    match source {
        SocketAddr::V4(v4) => {
            header.push(0x10 | transport);
            header.extend_from_slice(&12_u16.to_be_bytes());
            header.extend_from_slice(&v4.ip().octets());
            header.extend_from_slice(&[10, 0, 0, 1]);
            header.extend_from_slice(&v4.port().to_be_bytes());
            header.extend_from_slice(&53_u16.to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            header.push(0x20 | transport);
            header.extend_from_slice(&36_u16.to_be_bytes());
            header.extend_from_slice(&v6.ip().octets());
            header.extend_from_slice(&[0_u8; 16]);
            header.extend_from_slice(&v6.port().to_be_bytes());
            header.extend_from_slice(&53_u16.to_be_bytes());
        }
    }
    header
}
