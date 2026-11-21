use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde_json::json;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_core::status::HttpStatus;
use whl_store::captures::{CapturePolicy, NameValue};
use whl_store::import::{
    ImportCapture, ImportOutcome, ImportPlacement, ImportStop, ImportedBatch, storage_headroom,
};
use whl_store::{CaptureKind, EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const BASE: i64 = 1_700_000_000;

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
                overflow: Overflow::Refuse,
                tags: Vec::new(),
                verifier: None,
                templating: Default::default(),
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

fn policy(max_captures: u32) -> CapturePolicy {
    CapturePolicy {
        max_captures,
        retention: RETENTION,
    }
}

fn placement(keep_times: bool) -> ImportPlacement {
    ImportPlacement {
        keep_times,
        imported_at: OffsetDateTime::from_unix_timestamp(BASE + 1000).expect("a valid timestamp"),
    }
}

fn item(index: i64) -> ImportCapture {
    ImportCapture {
        original_id: CaptureId::generate().expect("an id is generated"),
        received_at: OffsetDateTime::from_unix_timestamp(BASE + index).expect("a valid timestamp"),
        kind: CaptureKind::Http,
        method: "POST".to_owned(),
        url: "https://whl.test/x".to_owned(),
        path: "/x".to_owned(),
        query_string: String::new(),
        host: "whl.test".to_owned(),
        client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
        headers: vec![NameValue {
            name: "x-test".to_owned(),
            value: "value-1".to_owned(),
        }],
        query: Vec::new(),
        form: None,
        content_type: Some("text/plain".to_owned()),
        body: b"hello".to_vec(),
        status_override: None,
        response_status: HttpStatus::new(200).expect("a valid status"),
        meta: json!({"hop": 1}),
    }
}

fn stored(outcome: ImportOutcome) -> ImportedBatch {
    match outcome {
        ImportOutcome::Stored(batch) => batch,
        ImportOutcome::Gone => panic!("the endpoint is gone"),
    }
}

#[tokio::test]
async fn imported_captures_get_new_ids_consecutive_seqs_and_the_imported_mark() {
    let fixture = fixture().await;
    let items = vec![item(1), item(2)];
    let originals: Vec<String> = items
        .iter()
        .map(|item| item.original_id.to_string())
        .collect();

    let batch = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                items,
                placement(true),
                policy(10),
                u64::MAX,
            )
            .await
            .expect("the batch is stored"),
    );

    assert_eq!((batch.imported, batch.stop), (2, None));
    let rows: Vec<(i64, String, bool, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT seq, id::text, imported, meta::text, received_at FROM captures \
         WHERE endpoint_id = $1 ORDER BY seq",
    )
    .bind(fixture.endpoint.unpack())
    .fetch_all(fixture._db.pool())
    .await
    .expect("the captures are read");
    assert_eq!(rows.iter().map(|row| row.0).collect::<Vec<_>>(), vec![1, 2]);
    for (row, original) in rows.iter().zip(&originals) {
        let meta: serde_json::Value = serde_json::from_str(&row.3).expect("the meta is JSON");
        assert_ne!(&row.1, original);
        assert!(row.2);
        assert_eq!(meta["original_id"], original.as_str());
        assert_eq!(meta["hop"], 1);
    }
    assert_eq!(rows[0].4.unix_timestamp(), BASE + 1);
    let (count, bytes, last_seq): (i32, i64, i64) =
        sqlx::query_as("SELECT capture_count, bytes_used, last_seq FROM endpoints WHERE id = $1")
            .bind(fixture.endpoint.unpack())
            .fetch_one(fixture._db.pool())
            .await
            .expect("the endpoint row is read");
    assert_eq!((count, last_seq), (2, 2));
    assert_eq!(u64::try_from(bytes).ok(), Some(batch.stored_bytes));
}

#[tokio::test]
async fn a_batch_stops_at_the_endpoint_capture_cap() {
    let fixture = fixture().await;

    let batch = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(1), item(2), item(3)],
                placement(true),
                policy(2),
                u64::MAX,
            )
            .await
            .expect("the batch is stored"),
    );
    let again = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(4)],
                placement(true),
                policy(2),
                u64::MAX,
            )
            .await
            .expect("the second batch is stored"),
    );

    assert_eq!(
        (batch.imported, batch.stop),
        (2, Some(ImportStop::Capacity))
    );
    assert_eq!(
        (again.imported, again.stop),
        (0, Some(ImportStop::Capacity))
    );
}

#[tokio::test]
async fn a_batch_stops_when_the_byte_budget_is_spent() {
    let fixture = fixture().await;
    let probe = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(1)],
                placement(true),
                policy(10),
                u64::MAX,
            )
            .await
            .expect("the probe is stored"),
    );
    let one = probe.stored_bytes;

    let batch = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(2), item(3), item(4)],
                placement(true),
                policy(10),
                one * 2 + one / 2,
            )
            .await
            .expect("the batch is stored"),
    );

    assert_eq!((batch.imported, batch.stop), (2, Some(ImportStop::Storage)));
    assert_eq!(batch.stored_bytes, one * 2);
}

#[tokio::test]
async fn keeping_times_off_stamps_every_capture_with_the_import_time() {
    let fixture = fixture().await;

    stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(1)],
                placement(false),
                policy(10),
                u64::MAX,
            )
            .await
            .expect("the batch is stored"),
    );

    let at: OffsetDateTime =
        sqlx::query_scalar("SELECT received_at FROM captures WHERE endpoint_id = $1")
            .bind(fixture.endpoint.unpack())
            .fetch_one(fixture._db.pool())
            .await
            .expect("the capture is read");
    assert_eq!(at.unix_timestamp(), BASE + 1000);
}

#[tokio::test]
async fn importing_into_an_unknown_endpoint_reports_it_gone() {
    let fixture = fixture().await;
    let unknown = EndpointId::parse("000000000000000000000000").expect("a valid id");

    let outcome = fixture
        .store
        .import_batch(
            &unknown,
            vec![item(1)],
            placement(true),
            policy(10),
            u64::MAX,
        )
        .await
        .expect("the batch is attempted");

    assert_eq!(outcome, ImportOutcome::Gone);
}

#[tokio::test]
async fn the_metadata_counts_towards_the_stored_bytes() {
    let fixture = fixture().await;
    let plain = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![item(1)],
                placement(true),
                policy(10),
                u64::MAX,
            )
            .await
            .expect("the plain capture is stored"),
    );
    let mut padded = item(2);
    padded.meta = json!({"pad": "x".repeat(1000)});

    let heavy = stored(
        fixture
            .store
            .import_batch(
                &fixture.endpoint,
                vec![padded],
                placement(true),
                policy(10),
                u64::MAX,
            )
            .await
            .expect("the padded capture is stored"),
    );

    assert!(heavy.stored_bytes >= plain.stored_bytes + 1000);
}

#[test]
fn the_approximate_size_of_a_capture_counts_its_text_and_body() {
    let small = item(1);
    let mut large = item(1);
    large.body = vec![0; 5000];
    assert!(large.approx_bytes() >= small.approx_bytes() + 4995);
}

#[test]
fn the_storage_headroom_is_what_is_left_under_the_cap() {
    assert_eq!(storage_headroom(30, 100), 70);
    assert_eq!(storage_headroom(100, 100), 0);
    assert_eq!(storage_headroom(150, 100), 0);
}
