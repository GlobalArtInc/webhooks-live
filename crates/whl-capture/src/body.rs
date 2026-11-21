use axum::body::Body;
use http_body_util::{BodyExt, LengthLimitError, Limited};
use tokio::sync::OwnedSemaphorePermit;

use crate::state::{BodyBudget, CaptureLimits};

const CHUNKED_INITIAL_RESERVATION: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadFailure {
    #[error("the body is larger than the limit")]
    TooLarge,
    #[error("the body was not received within the read timeout")]
    Timeout,
    #[error("the body stream failed before it ended")]
    Aborted,
    #[error("the body memory budget is exhausted")]
    BudgetExhausted,
}

#[derive(Debug)]
pub struct BufferedBody {
    pub data: Vec<u8>,
    pub reservation: Option<OwnedSemaphorePermit>,
}

pub async fn read_body(
    body: Body,
    declared: Option<u64>,
    limits: &CaptureLimits,
    budget: &BodyBudget,
) -> Result<BufferedBody, ReadFailure> {
    let mut reservation = None;
    let collected = tokio::time::timeout(
        limits.body_read_timeout,
        collect(body, declared, limits, budget, &mut reservation),
    )
    .await
    .map_err(|_| ReadFailure::Timeout)??;
    Ok(BufferedBody {
        data: collected,
        reservation,
    })
}

async fn collect(
    body: Body,
    declared: Option<u64>,
    limits: &CaptureLimits,
    budget: &BodyBudget,
    reservation: &mut Option<OwnedSemaphorePermit>,
) -> Result<Vec<u8>, ReadFailure> {
    let limit = limits.max_body_bytes;
    let initial = declared
        .map_or(CHUNKED_INITIAL_RESERVATION, |length| {
            usize::try_from(length).unwrap_or(limit)
        })
        .min(limit);
    let mut reserved = 0;
    grow_reservation(budget, limits, reservation, &mut reserved, initial).await?;
    let mut buffer = Vec::with_capacity(reserved);

    let mut body = Limited::new(body, limit);
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| {
            if error.downcast_ref::<LengthLimitError>().is_some() {
                ReadFailure::TooLarge
            } else {
                ReadFailure::Aborted
            }
        })?;
        let Ok(data) = frame.into_data() else {
            continue;
        };
        let needed = buffer.len() + data.len();
        if needed > limit {
            return Err(ReadFailure::TooLarge);
        }
        if needed > reserved {
            let target = needed.max(reserved.saturating_mul(2)).min(limit);
            grow_reservation(budget, limits, reservation, &mut reserved, target).await?;
            buffer.reserve_exact(reserved - buffer.len());
        }
        buffer.extend_from_slice(&data);
    }
    Ok(buffer)
}

async fn grow_reservation(
    budget: &BodyBudget,
    limits: &CaptureLimits,
    reservation: &mut Option<OwnedSemaphorePermit>,
    reserved: &mut usize,
    target: usize,
) -> Result<(), ReadFailure> {
    if target <= *reserved {
        return Ok(());
    }
    let extra = budget
        .reserve(target - *reserved, limits.budget_wait)
        .await
        .map_err(|_| ReadFailure::BudgetExhausted)?;
    match (reservation.as_mut(), extra) {
        (Some(held), Some(more)) => held.merge(more),
        (None, more) => *reservation = more,
        (Some(_), None) => {}
    }
    *reserved = target;
    Ok(())
}
