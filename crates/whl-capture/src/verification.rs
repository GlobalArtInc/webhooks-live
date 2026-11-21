use axum::http::HeaderMap;
use bytes::Bytes;
use time::OffsetDateTime;
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{SealedSecret, Secret};
use whl_store::captures::VerifierSource;
use whl_telemetry::series;
use whl_verify::{SignedRequest, Verdict, Verification, Verifier};

use crate::state::CaptureState;

pub const INLINE_VERIFY_BYTES: usize = 64 * 1024;

pub(crate) struct Presented<'a> {
    pub headers: &'a HeaderMap,
    pub body: &'a Bytes,
    pub received_at: OffsetDateTime,
}

pub(crate) async fn verify_capture(
    state: &CaptureState,
    endpoint: &EndpointId,
    source: &VerifierSource,
    presented: Presented<'_>,
) -> Verification {
    let verdict = match source {
        VerifierSource::Unusable => {
            tracing::warn!(endpoint = %endpoint, "the stored verifier cannot be read");
            Verdict::unusable_secret()
        }
        VerifierSource::Ready { verifier, sealed } => {
            if verifier.signature_present(presented.headers) {
                match open_secret(state, endpoint, verifier, sealed.as_ref()) {
                    Some(secret) => run(verifier, secret, &presented).await,
                    None => Verdict::unusable_secret(),
                }
            } else {
                Verdict::missing()
            }
        }
    };
    metrics::counter!(
        series::VERIFICATIONS_TOTAL,
        "result" => verdict.verification.as_str(),
        "reason" => verdict.reason.as_str(),
    )
    .increment(1);
    verdict.verification
}

fn open_secret(
    state: &CaptureState,
    endpoint: &EndpointId,
    verifier: &Verifier,
    sealed: Option<&SealedSecret>,
) -> Option<Secret> {
    let Some(keyring) = state.secrets() else {
        tracing::warn!(endpoint = %endpoint, "a verifier is configured but the secrets keyring is not");
        return None;
    };
    let Some(sealed) = sealed else {
        tracing::warn!(
            endpoint = %endpoint,
            secret = %verifier.secret_name(),
            "the secret of the verifier is missing"
        );
        return None;
    };
    match keyring.open(endpoint, verifier.secret_name(), sealed) {
        Ok(secret) => Some(secret),
        Err(error) => {
            tracing::warn!(
                endpoint = %endpoint,
                secret = %verifier.secret_name(),
                %error,
                "the secret of the verifier cannot be opened"
            );
            None
        }
    }
}

async fn run(verifier: &Verifier, secret: Secret, presented: &Presented<'_>) -> Verdict {
    if presented.body.len() <= INLINE_VERIFY_BYTES {
        return check(
            verifier,
            &secret,
            presented.headers,
            presented.body,
            presented.received_at,
        );
    }
    let verifier = verifier.clone();
    let headers = presented.headers.clone();
    let body = presented.body.clone();
    let received_at = presented.received_at;
    let job = tokio::task::spawn_blocking(move || {
        check(&verifier, &secret, &headers, &body, received_at)
    });
    match job.await {
        Ok(verdict) => verdict,
        Err(error) => {
            tracing::error!(%error, "the signature check task failed");
            Verdict::internal()
        }
    }
}

fn check(
    verifier: &Verifier,
    secret: &Secret,
    headers: &HeaderMap,
    body: &[u8],
    received_at: OffsetDateTime,
) -> Verdict {
    verifier.verify(
        secret,
        &SignedRequest {
            headers,
            body,
            received_at,
        },
    )
}
