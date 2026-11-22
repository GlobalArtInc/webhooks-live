use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use serde::Serialize;
use utoipa::ToSchema;
use whl_auth::Caller;
use whl_core::endpoint_id::EndpointId;
use whl_store::merge::merge_lanes;
use whl_store::stream::CaptureSummary;
use whl_store::{CaptureQuery, Order};

use super::cursor::{InboxCursor, LanePosition};
use super::selection::{SelectionRequest, select};
use crate::captures::{ListParams, SummaryDto, inbox_query};
use crate::error::{ApiError, ErrorBody};
use crate::openapi::responses::{StoreUnavailable, ValidationFailed};
use crate::state::ApiState;

#[derive(Serialize, ToSchema)]
#[schema(as = InboxList)]
pub struct InboxListDto {
    items: Vec<SummaryDto>,
    next_cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/inbox/captures",
    operation_id = "inbox_list",
    tag = "Inbox",
    summary = "Newest captures of several endpoints in one list",
    description = "Merges the captures of the selected endpoints, newest first; each item is a capture summary as in `GET /endpoints/{id}/captures`, with `endpoint_id`. Selection: `endpoints` (comma-separated ids, at most 50 signed in and 20 anonymous). Without it a signed-in caller gets all own and shared endpoints, at most 50; `open` requires `endpoints`. `folder` is refused with 422 until folders are selectable. An explicit selection with an endpoint that is unknown or that the caller may not view answers 410 `endpoint_gone`; the default selection shows only what the caller may view. Filters: `method`, `status`, `kind`, `q` (1 to 200 characters), `since` and `until`; `order` may only be `desc`, and `after_seq` answers 422 `not_allowed`. Session only: a Bearer token answers 403 `session_required`. A view-protected anonymous endpoint needs its grant in the `X-Webhooks-Live-Grant` header; the `?grant=` query parameter is not read by this route.",
    security(("session_cookie" = []), ()),
    params(
        ("endpoints" = Option<String>, Query, description = "Comma-separated endpoint ids"),
        ("folder" = Option<String>, Query, description = "Folder id; refused with 422 until folders are selectable"),
        ("order" = Option<String>, Query, pattern = "^desc$", description = "Only `desc`, newest first"),
        ("limit" = Option<u32>, Query, minimum = 1, maximum = 50, description = "Page size; default 25"),
        ("cursor" = Option<String>, Query, description = "From `next_cursor`; opaque, bound to the filters and the selection"),
        ("method" = Option<String>, Query, description = "Comma-separated HTTP methods, e.g. `POST,PUT`"),
        ("status" = Option<String>, Query, description = "Comma-separated status classes `1xx` to `5xx`, or `none`"),
        ("kind" = Option<String>, Query, description = "Comma-separated capture kinds"),
        ("q" = Option<String>, Query, min_length = 1, max_length = 200, description = "Case-insensitive substring of the capture"),
        ("since" = Option<String>, Query, format = DateTime, description = "Captures received at or after this time"),
        ("until" = Option<String>, Query, format = DateTime, description = "Captures received before this time"),
        ("X-Webhooks-Live-Grant" = Option<String>, Header, description = "View grant of view-protected anonymous endpoints of the selection")
    ),
    responses(
        (status = 200, description = "One page of captures, newest first", body = InboxListDto),
        (status = 400, description = "`invalid_cursor`: the cursor is malformed or was issued for other filters or another selection", body = ErrorBody),
        (status = 401, description = "`unauthenticated` or `invalid_token`", body = ErrorBody),
        (status = 403, description = "`session_required`: a Bearer token was used; `sign_in_required`: no `endpoints` and no signed-in caller in `mixed`", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: an endpoint of an explicit selection is unknown or not viewable", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
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
            headers: &headers,
            grant_query: None,
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
