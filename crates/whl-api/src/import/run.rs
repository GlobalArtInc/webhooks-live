use axum::body::Body;
use serde_json::{Map, Value};
use time::macros::datetime;
use time::{Duration, OffsetDateTime};
use whl_auth::{Caller, Creation};
use whl_core::endpoint_id::EndpointId;
use whl_export::{ImportError, ImportFormat, ImportLimits, ImportReader, Problem};
use whl_store::captures::CapturePolicy;
use whl_store::import::{
    IMPORT_BATCH, ImportCapture, ImportOutcome, ImportPlacement, ImportStop, storage_headroom,
};
use whl_telemetry::series;

use super::upload::{UploadError, guarded};
use super::{MAX_IMPORT_BYTES, MAX_IMPORT_DURATION, UPLOAD_IDLE_TIMEOUT};
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::handlers::store_new_endpoint;
use crate::input::{ParsedInput, parse_endpoint_input};
use crate::state::ApiState;

const ENDPOINT_KEYS: &[&str] = &["title", "response", "cors", "overflow"];
const MAX_IMPORT_STORED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const ENDPOINT_PREFIX: &str = "endpoint";
const SKEW: Duration = Duration::minutes(5);
const EARLIEST: OffsetDateTime = datetime!(2015-01-01 0:00 UTC);

pub enum Target {
    Existing(EndpointId),
    New,
}

#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub keep_times: bool,
}

#[derive(Debug)]
pub struct Outcome {
    pub endpoint: EndpointId,
    pub imported: usize,
    pub skipped: usize,
    pub stop: Option<ImportStop>,
}

#[derive(Default)]
struct Tally {
    imported: usize,
    skipped: usize,
    stored_bytes: u64,
    stop: Option<ImportStop>,
}

#[derive(Default)]
struct Progress {
    endpoint: Option<EndpointId>,
    created: bool,
    tally: Tally,
}

#[derive(Default)]
struct Batch {
    items: Vec<ImportCapture>,
    bytes: usize,
}

impl Batch {
    fn push(&mut self, capture: ImportCapture) {
        self.bytes = self.bytes.saturating_add(capture.approx_bytes());
        self.items.push(capture);
    }

    fn is_full(&self) -> bool {
        self.items.len() >= IMPORT_BATCH || self.bytes >= MAX_BATCH_BYTES
    }

    fn take(&mut self) -> Vec<ImportCapture> {
        self.bytes = 0;
        std::mem::take(&mut self.items)
    }
}

pub async fn execute(
    state: &ApiState,
    caller: &Caller,
    target: Target,
    placement: Placement,
    format: ImportFormat,
    body: Body,
) -> Result<Outcome, ApiError> {
    let mut progress = Progress::default();
    let work = pipeline(
        state,
        caller,
        target,
        placement,
        format,
        body,
        &mut progress,
    );
    let result = match tokio::time::timeout(MAX_IMPORT_DURATION, work).await {
        Ok(result) => result,
        Err(_) => Err(ApiError::InvalidJson(
            "the import ran longer than its time limit",
        )),
    };
    match result {
        Ok(endpoint) => {
            announce(state, &endpoint, progress.tally.imported).await;
            Ok(Outcome {
                endpoint,
                imported: progress.tally.imported,
                skipped: progress.tally.skipped,
                stop: progress.tally.stop,
            })
        }
        Err(error) => {
            if let Some(endpoint) = &progress.endpoint {
                if progress.created {
                    discard(state, endpoint).await;
                } else {
                    announce(state, endpoint, progress.tally.imported).await;
                }
            }
            Err(error)
        }
    }
}

async fn pipeline(
    state: &ApiState,
    caller: &Caller,
    target: Target,
    placement: Placement,
    format: ImportFormat,
    body: Body,
    progress: &mut Progress,
) -> Result<EndpointId, ApiError> {
    let now = OffsetDateTime::now_utc();
    let limits = ImportLimits {
        max_input_bytes: MAX_IMPORT_BYTES,
        max_body_bytes: state.settings.max_body_bytes,
        earliest: EARLIEST,
        latest: now + SKEW,
        enforce_time_range: placement.keep_times,
    };
    let stream = guarded(body, UPLOAD_IDLE_TIMEOUT);
    let (mut reader, header) = ImportReader::open(stream, format, limits)
        .await
        .map_err(import_error)?;
    let endpoint = match target {
        Target::Existing(endpoint) => endpoint,
        Target::New => {
            let parsed = endpoint_input(header.endpoint)?;
            let creation = state.access.creation(caller)?;
            let record = create_endpoint(state, caller, &creation, parsed).await?;
            progress.created = true;
            record.id
        }
    };
    progress.endpoint = Some(endpoint.clone());
    ingest(
        state,
        &mut reader,
        &endpoint,
        ImportPlacement {
            keep_times: placement.keep_times,
            imported_at: now,
        },
        &mut progress.tally,
    )
    .await?;
    Ok(endpoint)
}

async fn ingest<S, B, E>(
    state: &ApiState,
    reader: &mut ImportReader<S>,
    endpoint: &EndpointId,
    placement: ImportPlacement,
    tally: &mut Tally,
) -> Result<(), ApiError>
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::error::Error + Send + Sync + 'static,
{
    let mut batch = Batch::default();
    loop {
        let next = reader.next_capture().await.map_err(import_error)?;
        match next {
            Some(capture) if tally.stop.is_some() => {
                drop(capture);
                tally.skipped += 1;
            }
            Some(capture) => {
                batch.push(capture);
                if batch.is_full() {
                    flush(state, endpoint, &mut batch, placement, tally).await?;
                }
            }
            None => {
                if !batch.items.is_empty() {
                    flush(state, endpoint, &mut batch, placement, tally).await?;
                }
                return Ok(());
            }
        }
    }
}

async fn flush(
    state: &ApiState,
    endpoint: &EndpointId,
    batch: &mut Batch,
    placement: ImportPlacement,
    tally: &mut Tally,
) -> Result<(), ApiError> {
    let captures = batch.take();
    let size = captures.len();
    let used = state.store.storage_bytes().await.map_err(|error| {
        tracing::error!(%error, "reading the storage figure for an import failed");
        ApiError::StoreUnavailable
    })?;
    let free = storage_headroom(used, state.settings.storage_cap_bytes);
    let allowance = MAX_IMPORT_STORED_BYTES.saturating_sub(tally.stored_bytes);
    let policy = CapturePolicy {
        max_captures: state.settings.max_captures,
        retention: state.settings.retention,
    };
    let outcome = state
        .store
        .import_batch(endpoint, captures, placement, policy, free.min(allowance))
        .await?;
    let ImportOutcome::Stored(stored) = outcome else {
        return Err(ApiError::EndpointGone);
    };
    tally.stored_bytes = tally.stored_bytes.saturating_add(stored.stored_bytes);
    tally.imported += stored.imported;
    tally.skipped += size - stored.imported;
    metrics::counter!(series::CAPTURES_IMPORTED_TOTAL).increment(stored.imported as u64);
    if let Some(stop) = stored.stop
        && tally.stop.is_none()
    {
        tally.stop = Some(stop);
        metrics::counter!(series::IMPORT_STOPS_TOTAL, "reason" => stop.as_str()).increment(1);
    }
    Ok(())
}

fn endpoint_input(endpoint: Map<String, Value>) -> Result<ParsedInput, ApiError> {
    if let Some(key) = endpoint
        .keys()
        .find(|key| !ENDPOINT_KEYS.contains(&key.as_str()))
    {
        let shown: String = key.chars().take(64).collect();
        return Err(ApiError::Validation(vec![
            FieldViolation::new(shown, FieldCode::NotAllowed, "is not a known field")
                .prefixed(ENDPOINT_PREFIX),
        ]));
    }
    let bytes = serde_json::to_vec(&Value::Object(endpoint)).map_err(|error| {
        tracing::error!(%error, "an import endpoint object could not be encoded");
        ApiError::InvalidJson("the endpoint object cannot be read")
    })?;
    parse_endpoint_input(&bytes, false).map_err(prefix_endpoint)
}

async fn create_endpoint(
    state: &ApiState,
    caller: &Caller,
    creation: &Creation,
    parsed: ParsedInput,
) -> Result<whl_store::EndpointRecord, ApiError> {
    store_new_endpoint(state, caller, creation, parsed)
        .await
        .map_err(prefix_endpoint)
}

fn prefix_endpoint(error: ApiError) -> ApiError {
    match error {
        ApiError::Validation(violations) => ApiError::Validation(
            violations
                .into_iter()
                .map(|violation| violation.prefixed(ENDPOINT_PREFIX))
                .collect(),
        ),
        other => other,
    }
}

async fn discard(state: &ApiState, endpoint: &EndpointId) {
    if let Err(error) = state
        .store
        .delete_endpoint(endpoint, state.settings.retention)
        .await
    {
        tracing::warn!(%error, endpoint = endpoint.unpack(), "removing the endpoint of a failed import failed");
    }
}

async fn announce(state: &ApiState, endpoint: &EndpointId, imported: usize) {
    if imported == 0 {
        return;
    }
    if let Err(error) = state.store.announce_import(endpoint, imported).await {
        tracing::warn!(%error, endpoint = endpoint.unpack(), "announcing an import failed");
    }
}

fn import_error(error: ImportError) -> ApiError {
    match error {
        ImportError::TooLarge { .. } => ApiError::ImportTooLarge,
        ImportError::Read(source) => {
            if let Some(upload) = source.downcast_ref::<UploadError>() {
                tracing::debug!(%upload, "an import upload ended early");
            } else {
                tracing::debug!(%source, "an import could not be read");
            }
            ApiError::InvalidJson("the request body could not be read")
        }
        ImportError::Invalid(violation) => ApiError::Validation(vec![FieldViolation::new(
            violation.field(),
            field_code(violation.problem),
            violation.message(),
        )]),
    }
}

fn field_code(problem: Problem) -> FieldCode {
    match problem {
        Problem::Required => FieldCode::Required,
        Problem::InvalidFormat => FieldCode::InvalidFormat,
        Problem::TooLong => FieldCode::TooLong,
        Problem::OutOfRange => FieldCode::OutOfRange,
        Problem::TooMany => FieldCode::TooMany,
        Problem::NotAllowed => FieldCode::NotAllowed,
    }
}
