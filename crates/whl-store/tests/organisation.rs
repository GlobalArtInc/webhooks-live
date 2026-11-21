use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use bytes::Bytes;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::captures::{CaptureOutcome, CapturePolicy, NewCapture};
use whl_store::{CaptureAnnotation, EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const CAP: u32 = 2;

struct Fixture {
    _db: TestDb,
    store: Store,
    endpoint: EndpointId,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let record = store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                cors: false,
                overflow: Overflow::Ring,
                tags: Vec::new(),
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    Fixture {
        _db: db,
        store,
        endpoint: record.id,
    }
}

impl Fixture {
    async fn capture(&self) -> (CaptureId, CaptureOutcome) {
        let id = CaptureId::generate().expect("an id is generated");
        let outcome = self
            .store
            .store_capture(
                NewCapture {
                    id,
                    endpoint_id: self.endpoint.clone(),
                    received_at: OffsetDateTime::now_utc(),
                    method: "POST".to_owned(),
                    url: "http://h/x".to_owned(),
                    path: "/x".to_owned(),
                    query_string: String::new(),
                    host: "h".to_owned(),
                    client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
                    headers: Vec::new(),
                    query: Vec::new(),
                    form: None,
                    content_type: Some("text/plain".to_owned()),
                    body: Bytes::from_static(b"hello"),
                    status_override: None,
                },
                CapturePolicy {
                    max_captures: CAP,
                    retention: RETENTION,
                },
            )
            .await
            .expect("the capture is processed");
        (id, outcome)
    }

    async fn star(&self, capture: &CaptureId) {
        let annotation = CaptureAnnotation {
            starred: Some(true),
            ..CaptureAnnotation::default()
        };
        self.store
            .annotate_capture(&self.endpoint, capture, &annotation, RETENTION)
            .await
            .expect("the capture is annotated");
    }

    async fn exists(&self, capture: &CaptureId) -> bool {
        self.store
            .get_capture(&self.endpoint, capture)
            .await
            .expect("the capture is read")
            .is_some()
    }
}

#[tokio::test]
async fn ring_eviction_skips_a_starred_capture() {
    let fixture = fixture().await;
    let (starred, _) = fixture.capture().await;
    fixture.star(&starred).await;
    let (evicted, _) = fixture.capture().await;
    let (_, outcome) = fixture.capture().await;
    assert!(matches!(outcome, CaptureOutcome::Stored { .. }));
    assert!(fixture.exists(&starred).await);
    assert!(!fixture.exists(&evicted).await);
}

#[tokio::test]
async fn ring_refuses_when_every_retained_capture_is_starred() {
    let fixture = fixture().await;
    let (first, _) = fixture.capture().await;
    fixture.star(&first).await;
    let (second, _) = fixture.capture().await;
    fixture.star(&second).await;
    let (_, outcome) = fixture.capture().await;
    assert!(matches!(outcome, CaptureOutcome::Full));
    assert!(fixture.exists(&first).await);
    assert!(fixture.exists(&second).await);
}
