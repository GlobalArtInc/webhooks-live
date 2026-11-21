use whl_secrets::{Keyring, SealError};
use whl_store::Store;
use whl_store::secrets::{
    ReplaceSealedOutcome, RotationCandidate, RotationCursor, SecretStoreError,
};

pub const DEFAULT_BATCH_SIZE: u32 = 100;
pub const MAX_BATCH_SIZE: u32 = 1000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RotateReport {
    pub batches: u64,
    pub rotated: u64,
    pub skipped: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotateProgress {
    pub pending_at_start: u64,
    pub report: RotateReport,
}

#[derive(Debug, thiserror::Error)]
pub enum RotateError {
    #[error("the batch size must be between 1 and {MAX_BATCH_SIZE}")]
    BatchSize,
    #[error("the secret store failed: {0}")]
    Store(#[from] SecretStoreError),
    #[error("re-sealing failed: {0}")]
    Seal(#[from] SealError),
}

enum Step {
    Rotated,
    Skipped,
    Failed,
}

pub async fn rotate_secrets(
    store: &Store,
    keyring: &Keyring,
    batch_size: u32,
    mut progress: impl FnMut(&RotateProgress),
) -> Result<RotateReport, RotateError> {
    if !(1..=MAX_BATCH_SIZE).contains(&batch_size) {
        return Err(RotateError::BatchSize);
    }
    let active = keyring.active_id();
    let pending_at_start = store.count_secrets_outside_key(active).await?;
    let mut report = RotateReport::default();
    let mut cursor = RotationCursor::start();
    loop {
        let batch = store.rotation_batch(active, &cursor, batch_size).await?;
        let Some(last) = batch.last() else {
            return Ok(report);
        };
        cursor = RotationCursor {
            endpoint: last.endpoint.clone(),
            name: last.name.clone(),
            channel: last.channel,
        };
        let fetched = batch.len();
        for candidate in &batch {
            match rotate_one(store, keyring, candidate).await? {
                Step::Rotated => report.rotated += 1,
                Step::Skipped => report.skipped += 1,
                Step::Failed => report.failed += 1,
            }
        }
        report.batches += 1;
        progress(&RotateProgress {
            pending_at_start,
            report,
        });
        if fetched < usize::try_from(batch_size).unwrap_or(usize::MAX) {
            return Ok(report);
        }
    }
}

async fn rotate_one(
    store: &Store,
    keyring: &Keyring,
    candidate: &RotationCandidate,
) -> Result<Step, RotateError> {
    let Some(stored) = &candidate.stored else {
        tracing::warn!(
            endpoint = %candidate.endpoint,
            secret = %candidate.name,
            "the stored secret row is malformed and was left untouched"
        );
        return Ok(Step::Failed);
    };
    let value = match keyring.open(&stored.endpoint, &stored.name, &stored.sealed) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(
                endpoint = %stored.endpoint,
                secret = %stored.name,
                key_id = %stored.sealed.key_id(),
                %error,
                "the stored secret cannot be opened with the configured keys and was left untouched"
            );
            return Ok(Step::Failed);
        }
    };
    let replacement = keyring.seal(&stored.endpoint, &stored.name, &value)?;
    let verified = keyring
        .open(&stored.endpoint, &stored.name, &replacement)
        .is_ok_and(|check| check == value);
    if !verified {
        tracing::error!(
            endpoint = %stored.endpoint,
            secret = %stored.name,
            "the re-sealed secret did not open back to the same value and was not written"
        );
        return Ok(Step::Failed);
    }
    let replaced = if candidate.channel {
        store
            .replace_sealed_channel_secret(stored, &replacement)
            .await?
    } else {
        store.replace_sealed_secret(stored, &replacement).await?
    };
    Ok(match replaced {
        ReplaceSealedOutcome::Replaced => Step::Rotated,
        ReplaceSealedOutcome::Changed => Step::Skipped,
    })
}
