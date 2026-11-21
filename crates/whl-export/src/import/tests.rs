use std::convert::Infallible;

use futures_util::stream;
use serde_json::{Value, json};
use time::macros::datetime;

use super::*;

const CAPTURE_ID: &str = "0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const ENDPOINT_ID: &str = "abcdefghjkmnpqrstvwxyz23";

fn limits() -> ImportLimits {
    ImportLimits {
        max_input_bytes: 1024 * 1024,
        max_body_bytes: 1024,
        earliest: datetime!(2015-01-01 0:00 UTC),
        latest: datetime!(2030-01-01 0:00 UTC),
        enforce_time_range: true,
    }
}

fn head() -> Value {
    json!({
        "format": "webhooks-live-export",
        "version": 1,
        "exported_at": "2026-11-21T09:05:07.000Z",
        "endpoint": {
            "id": ENDPOINT_ID,
            "title": null,
            "response": {
                "status": 200,
                "content_type": "text/plain",
                "headers": [],
                "body": {"encoding": "utf8", "data": "", "truncated": false},
                "delay_ms": 0
            },
            "cors": false,
            "overflow": "refuse"
        }
    })
}

fn capture() -> Value {
    json!({
        "id": CAPTURE_ID,
        "seq": 1,
        "kind": "http",
        "received_at": "2026-11-20T10:00:00.250Z",
        "method": "POST",
        "path": "/hook",
        "query_string": "a=1",
        "url": "https://whl.test/abcdefghjkmnpqrstvwxyz23/hook?a=1",
        "host": "whl.test",
        "client_ip": "203.0.113.7",
        "content_type": "text/plain",
        "body_size": 5,
        "status_override": null,
        "response_status": 200,
        "headers": [{"name": "x-test", "value": "value-1"}],
        "query": [{"name": "a", "value": "1"}],
        "form": null,
        "meta": {},
        "body": {"encoding": "utf8", "data": "hello", "truncated": false}
    })
}

fn changed(mutate: impl FnOnce(&mut Value)) -> Value {
    let mut value = capture();
    mutate(&mut value);
    value
}

fn with_captures(head: &Value, tail: &str) -> String {
    let mut text = head.to_string();
    text.pop();
    text.push_str(",\"captures\":[");
    text.push_str(tail);
    text.push_str("]}");
    text
}

fn document(captures: &[Value]) -> String {
    let joined = captures
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join(",");
    with_captures(&head(), &joined)
}

fn lines(captures: &[Value]) -> String {
    let mut text = format!("{}\n", head());
    for capture in captures {
        text.push_str(&format!("{capture}\n"));
    }
    text
}

fn chunks(
    text: &str,
    size: usize,
) -> impl stream::Stream<Item = Result<Vec<u8>, Infallible>> + Unpin {
    let parts: Vec<Result<Vec<u8>, Infallible>> = text
        .as_bytes()
        .chunks(size)
        .map(|part| Ok(part.to_vec()))
        .collect();
    stream::iter(parts)
}

async fn read_all(
    text: &str,
    format: ImportFormat,
    size: usize,
    limits: ImportLimits,
) -> Result<(ImportHeader, Vec<whl_store::import::ImportCapture>), ImportError> {
    let (mut reader, header) = ImportReader::open(chunks(text, size), format, limits).await?;
    let mut captures = Vec::new();
    while let Some(capture) = reader.next_capture().await? {
        captures.push(capture);
    }
    Ok((header, captures))
}

async fn failure(text: &str, format: ImportFormat) -> ImportViolation {
    match read_all(text, format, 4096, limits()).await {
        Err(ImportError::Invalid(violation)) => violation,
        other => panic!("expected a violation, got {other:?}"),
    }
}

#[tokio::test]
async fn a_json_document_yields_its_header_and_every_capture() {
    let text = document(&[capture(), capture()]);
    let (header, captures) = read_all(&text, ImportFormat::Json, 4096, limits())
        .await
        .expect("the document is read");
    assert_eq!(header.endpoint.get("id"), None);
    assert_eq!(header.endpoint["cors"], json!(false));
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].original_id.to_string(), CAPTURE_ID);
    assert_eq!(captures[0].body, b"hello");
    assert_eq!(captures[0].headers[0].name, "x-test");
}

#[tokio::test]
async fn an_ndjson_stream_skips_blank_lines_and_ends_without_a_final_newline() {
    let mut text = lines(&[capture()]);
    text.push_str("\n   \r\n");
    text.push_str(&capture().to_string());
    let (_, captures) = read_all(&text, ImportFormat::Ndjson, 4096, limits())
        .await
        .expect("the stream is read");
    assert_eq!(captures.len(), 2);
}

#[tokio::test]
async fn input_split_into_tiny_chunks_reads_the_same_as_one_chunk() {
    let text = document(&[capture(), changed(|value| value["path"] = json!("/other"))]);
    let whole = read_all(&text, ImportFormat::Json, text.len(), limits())
        .await
        .expect("one chunk reads");
    let split = read_all(&text, ImportFormat::Json, 3, limits())
        .await
        .expect("tiny chunks read");
    assert_eq!(whole.1.len(), split.1.len());
    assert_eq!(split.1[1].path, "/other");
    let text = lines(&[capture(), capture()]);
    let split = read_all(&text, ImportFormat::Ndjson, 5, limits())
        .await
        .expect("tiny chunks of lines read");
    assert_eq!(split.1.len(), 2);
}

#[tokio::test]
async fn a_base64_body_decodes_to_the_original_bytes() {
    let value = changed(|value| {
        value["body"] = json!({"encoding": "base64", "data": "AP8Q", "truncated": false});
        value["body_size"] = json!(3);
    });
    let (_, captures) = read_all(&document(&[value]), ImportFormat::Json, 4096, limits())
        .await
        .expect("the document is read");
    assert_eq!(captures[0].body, vec![0, 255, 16]);
}

#[tokio::test]
async fn a_capture_with_an_unknown_field_is_refused_at_its_index() {
    let bad = changed(|value| value["extra"] = json!(1));
    let violation = failure(&document(&[capture(), bad]), ImportFormat::Json).await;
    assert_eq!(violation.field(), "captures.1.extra");
    assert_eq!(violation.problem, Problem::NotAllowed);
}

#[tokio::test]
async fn an_ndjson_violation_names_the_line_counting_blank_lines() {
    let mut text = lines(&[capture()]);
    text.push('\n');
    text.push_str(&changed(|value| value["method"] = json!("GE T")).to_string());
    text.push('\n');
    let violation = failure(&text, ImportFormat::Ndjson).await;
    assert_eq!(violation.field(), "line.4");
    assert_eq!(violation.message(), "method must be a valid HTTP token");
}

#[tokio::test]
async fn the_header_must_name_the_format_and_version_one() {
    for (key, value, field) in [
        ("format", json!("other"), "format"),
        ("version", json!(2), "version"),
        ("exported_at", json!("yesterday"), "exported_at"),
    ] {
        let mut value_head = head();
        value_head[key] = value;
        let violation = failure(&with_captures(&value_head, ""), ImportFormat::Json).await;
        assert_eq!(violation.field(), field);
    }
}

#[tokio::test]
async fn captures_listed_before_the_header_is_complete_are_refused() {
    let text = r#"{"format":"webhooks-live-export","version":1,"captures":[]}"#;
    let violation = failure(text, ImportFormat::Json).await;
    assert_eq!(violation.field(), "exported_at");
    assert_eq!(violation.problem, Problem::Required);
}

#[tokio::test]
async fn an_endpoint_id_that_is_not_an_endpoint_id_is_refused() {
    let mut value = head();
    value["endpoint"]["id"] = json!("short");
    let violation = failure(&with_captures(&value, ""), ImportFormat::Json).await;
    assert_eq!(violation.field(), "endpoint.id");
}

#[tokio::test]
async fn input_beyond_the_size_limit_is_refused_while_reading() {
    let text = document(&vec![capture(); 8]);
    let mut small = limits();
    small.max_input_bytes = 500;
    let outcome = read_all(&text, ImportFormat::Json, 64, small).await;
    assert!(matches!(outcome, Err(ImportError::TooLarge { max: 500 })));
}

#[tokio::test]
async fn one_capture_longer_than_the_element_limit_is_refused() {
    let mut tight = limits();
    tight.max_body_bytes = 0;
    tight.max_input_bytes = 10 * 1024 * 1024;
    let padded =
        changed(|value| value["path"] = json!("p".repeat(ELEMENT_OVERHEAD_BYTES as usize)));
    let outcome = read_all(&document(&[padded]), ImportFormat::Json, 65536, tight).await;
    match outcome {
        Err(ImportError::Invalid(violation)) => assert_eq!(violation.problem, Problem::TooLong),
        other => panic!("expected a too long violation, got {other:?}"),
    }
}

#[tokio::test]
async fn a_body_that_does_not_match_its_declared_size_or_is_truncated_is_refused() {
    let wrong_size = changed(|value| value["body_size"] = json!(6));
    assert_eq!(
        failure(&document(&[wrong_size]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.body_size"
    );
    let truncated = changed(|value| value["body"]["truncated"] = json!(true));
    assert_eq!(
        failure(&document(&[truncated]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.body.truncated"
    );
    let oversized = changed(|value| {
        value["body"]["data"] = json!("x".repeat(1025));
        value["body_size"] = json!(1025);
    });
    let violation = failure(&document(&[oversized]), ImportFormat::Json).await;
    assert_eq!(violation.field(), "captures.0.body.data");
    assert_eq!(violation.problem, Problem::TooLong);
    let broken = changed(|value| {
        value["body"] = json!({"encoding": "base64", "data": "***", "truncated": false});
    });
    assert_eq!(
        failure(&document(&[broken]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.body.data"
    );
}

#[tokio::test]
async fn header_names_and_values_follow_the_http_rules() {
    let bad_name = changed(|value| value["headers"][0]["name"] = json!("bad name"));
    assert_eq!(
        failure(&document(&[bad_name]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.headers.0.name"
    );
    let bad_value = changed(|value| value["headers"][0]["value"] = json!("a\r\nb"));
    assert_eq!(
        failure(&document(&[bad_value]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.headers.0.value"
    );
    let too_many = changed(|value| {
        value["headers"] = Value::Array(vec![
            json!({"name": "x-a", "value": "1"});
            capture::MAX_PAIRS + 1
        ]);
    });
    let violation = failure(&document(&[too_many]), ImportFormat::Json).await;
    assert_eq!(violation.problem, Problem::TooMany);
}

#[tokio::test]
async fn ids_kinds_addresses_statuses_and_times_are_validated() {
    let cases = [
        ("id", json!("not-a-uuid")),
        ("kind", json!("sms")),
        ("client_ip", json!("999.1.1.1")),
        ("response_status", json!(99)),
        ("status_override", json!(600)),
        ("received_at", json!("2001-01-01T00:00:00Z")),
        ("received_at", json!("2031-01-01T00:00:00Z")),
        ("received_at", json!("last week")),
    ];
    for (field, replacement) in cases {
        let bad = changed(|value| value[field] = replacement.clone());
        let violation = failure(&document(&[bad]), ImportFormat::Json).await;
        assert_eq!(
            violation.field(),
            format!("captures.0.{field}"),
            "{replacement}"
        );
    }
}

#[tokio::test]
async fn metadata_nested_beyond_the_limit_or_not_an_object_is_refused() {
    let mut deep = json!({});
    for _ in 0..MAX_DEPTH {
        deep = json!({"a": deep});
    }
    let nested = changed(|value| value["meta"] = deep);
    let violation = failure(&lines(&[nested]), ImportFormat::Ndjson).await;
    assert_eq!(violation.problem, Problem::OutOfRange);
    let scalar = changed(|value| value["meta"] = json!(1));
    assert_eq!(
        failure(&document(&[scalar]), ImportFormat::Json)
            .await
            .field(),
        "captures.0.meta"
    );
}

#[tokio::test]
async fn a_document_nested_beyond_the_limit_is_refused_while_scanning() {
    let deep = with_captures(&head(), &"{\"a\":".repeat(MAX_DEPTH + 2));
    let violation = failure(&deep, ImportFormat::Json).await;
    assert_eq!(violation.problem, Problem::OutOfRange);
}

#[tokio::test]
async fn malformed_json_is_refused_with_its_position() {
    let mut text = document(&[capture()]);
    text.insert_str(text.len() - 3, "}}");
    let violation = failure(&text, ImportFormat::Json).await;
    assert_eq!(violation.problem, Problem::InvalidFormat);
    let cut = document(&[capture()]);
    let violation = failure(&cut[..cut.len() - 4], ImportFormat::Json).await;
    assert_eq!(violation.message(), "ends before the document is complete");
    let trailing = format!("{} x", document(&[capture()]));
    let violation = failure(&trailing, ImportFormat::Json).await;
    assert_eq!(
        violation.message(),
        "has data after the end of the document"
    );
    let broken = lines(&[capture()]) + "{\"id\": \n";
    let violation = failure(&broken, ImportFormat::Ndjson).await;
    assert_eq!(violation.field(), "line.3");
    assert!(violation.message().contains("near byte"));
}

#[tokio::test]
async fn content_types_select_the_format() {
    assert_eq!(
        ImportFormat::from_content_type("application/json; charset=utf-8"),
        Some(ImportFormat::Json)
    );
    assert_eq!(
        ImportFormat::from_content_type("Application/X-NDJSON"),
        Some(ImportFormat::Ndjson)
    );
    assert_eq!(ImportFormat::from_content_type("application/zip"), None);
}

#[tokio::test]
async fn a_document_without_the_capture_list_is_refused() {
    let text = head().to_string();
    let violation = failure(&text, ImportFormat::Json).await;
    assert_eq!(violation.field(), "captures");
    assert_eq!(violation.problem, Problem::Required);
}

#[tokio::test]
async fn times_outside_the_range_pass_when_the_range_is_not_enforced() {
    let early = changed(|value| value["received_at"] = json!("2001-01-01T00:00:00Z"));
    let mut relaxed = limits();
    relaxed.enforce_time_range = false;
    let (_, captures) = read_all(&document(&[early]), ImportFormat::Json, 4096, relaxed)
        .await
        .expect("the document is read");
    assert_eq!(captures.len(), 1);
    let broken = changed(|value| value["received_at"] = json!("last week"));
    let outcome = read_all(&document(&[broken]), ImportFormat::Json, 4096, relaxed).await;
    assert!(matches!(outcome, Err(ImportError::Invalid(_))));
}

#[tokio::test]
async fn header_names_are_lowercased_and_mapped_addresses_become_ipv4() {
    let value = changed(|value| {
        value["headers"][0]["name"] = json!("X-Mixed-Case");
        value["client_ip"] = json!("::ffff:203.0.113.7");
    });
    let (_, captures) = read_all(&document(&[value]), ImportFormat::Json, 4096, limits())
        .await
        .expect("the document is read");
    assert_eq!(captures[0].headers[0].name, "x-mixed-case");
    assert_eq!(captures[0].client_ip.to_string(), "203.0.113.7");
}

#[tokio::test]
async fn control_characters_in_the_path_host_url_and_query_string_are_refused() {
    for field in ["path", "host", "url", "query_string"] {
        let bad = changed(|value| value[field] = json!("a\u{1}b"));
        let violation = failure(&document(&[bad]), ImportFormat::Json).await;
        assert_eq!(violation.field(), format!("captures.0.{field}"));
    }
}

#[tokio::test]
async fn a_long_run_of_blank_lines_is_read_in_linear_time() {
    let mut text = lines(&[]);
    text.push_str(&"\n".repeat(50 * 1024 * 1024));
    text.push_str(&capture().to_string());
    let mut roomy = limits();
    roomy.max_input_bytes = 64 * 1024 * 1024;
    let started = std::time::Instant::now();
    let (_, captures) = read_all(&text, ImportFormat::Ndjson, 64 * 1024, roomy)
        .await
        .expect("the stream is read");
    assert_eq!(captures.len(), 1);
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}
