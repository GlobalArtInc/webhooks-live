use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use serde::Serialize;
use whl_auth::Caller;
use whl_core::endpoint_id::EndpointId;
use whl_store::merge::merge_lanes;
use whl_store::stream::CaptureSummary;
use whl_store::{CaptureQuery, Order};

use super::cursor::{InboxCursor, LanePosition};
use super::selection::{SelectionRequest, select};
use crate::captures::{ListParams, SummaryDto, inbox_query};
use crate::error::ApiError;
use crate::state::ApiState;

#[derive(Serialize)]
pub struct InboxListDto {
    items: Vec<SummaryDto>,
    next_cursor: Option<String>,
}

pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
    Query(params): Query<ListParams>,
) -> Result<Json<InboxListDto>, ApiError> {
    let query = inbox_query(&params)?;
    let fingerprint = query.filter.fingerprint();
    let held = match query.cursor.as_deref() {
        Some(text) => {
            let cursor = InboxCursor::decode(text).ok_or(ApiError::InvalidCursor)?;
            if !cursor.matches_filters(&fingerprint) {
                return Err(ApiError::InvalidCursor);
            }
            Some(cursor)
        }
        None => None,
    };
    let explicit = params.endpoints().is_some();
    let endpoints = select(
        &state.access,
        &state.store,
        state.settings.retention,
        &caller,
        SelectionRequest {
            endpoints: params.endpoints(),
            folder: params.folder(),
            resumed: held
                .as_ref()
                .filter(|_| !explicit)
                .map(InboxCursor::endpoints),
        },
    )
    .await?;
    if explicit
        && let Some(cursor) = &held
        && !cursor.belongs_to(&fingerprint, &endpoints)
    {
        return Err(ApiError::InvalidCursor);
    }

    let mut lanes: Vec<Vec<CaptureSummary>> = Vec::with_capacity(endpoints.len());
    let mut more: Vec<bool> = Vec::with_capacity(endpoints.len());
    let mut before: Vec<LanePosition> = Vec::with_capacity(endpoints.len());
    for endpoint in &endpoints {
        let position = held
            .as_ref()
            .and_then(|cursor| cursor.lane(endpoint))
            .unwrap_or(LanePosition::Top);
        before.push(position);
        let bound = match position {
            LanePosition::Exhausted => {
                lanes.push(Vec::new());
                more.push(false);
                continue;
            }
            LanePosition::Top => None,
            LanePosition::Before(seq) => Some(seq),
        };
        let slice = state
            .store
            .captures_slice(
                endpoint,
                &CaptureQuery {
                    filter: query.filter.clone(),
                    order: Order::Desc,
                    limit: query.limit,
                    position: bound,
                },
            )
            .await?;
        more.push(slice.next_position.is_some());
        lanes.push(slice.items);
    }

    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let merged = merge_lanes(&lanes, Order::Desc, limit);
    let next = next_positions(
        fingerprint,
        &endpoints,
        &lanes,
        &more,
        &before,
        &merged.taken,
    );
    let next_cursor = if next.is_exhausted() {
        None
    } else {
        Some(next.encode().ok_or(ApiError::InvalidCursor)?)
    };
    Ok(Json(InboxListDto {
        items: merged.items.iter().map(SummaryDto::from).collect(),
        next_cursor,
    }))
}

fn next_positions(
    fingerprint: String,
    endpoints: &[EndpointId],
    lanes: &[Vec<CaptureSummary>],
    more: &[bool],
    before: &[LanePosition],
    taken: &[usize],
) -> InboxCursor {
    let mut positions = BTreeMap::new();
    for (index, endpoint) in endpoints.iter().enumerate() {
        let remaining = lanes[index].len() - taken[index];
        let drained = before[index] == LanePosition::Exhausted || (remaining == 0 && !more[index]);
        let position = if drained {
            LanePosition::Exhausted
        } else if taken[index] > 0 {
            LanePosition::Before(lanes[index][taken[index] - 1].seq)
        } else {
            before[index]
        };
        positions.insert(endpoint.unpack().to_owned(), position);
    }
    InboxCursor::new(fingerprint, positions)
}
