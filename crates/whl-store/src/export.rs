use sqlx::postgres::PgRow;
use sqlx::types::Uuid;
use sqlx::{Postgres, QueryBuilder, Row};
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::capture_queries::{
    CaptureBody, CaptureDetail, CaptureError, CaptureFilter, CaptureForm, NameValue, classify,
    decode, push_filters,
};
use crate::stream::{summary_columns, summary_from_row};

#[derive(Debug, Clone, PartialEq)]
pub enum ExportSelection {
    Filter(CaptureFilter),
    Ids(Vec<Uuid>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportSnapshot {
    pub total_bytes: i64,
    pub last_seq: i64,
}

impl Store {
    pub async fn export_snapshot(
        &self,
        endpoint: &EndpointId,
        selection: &ExportSelection,
    ) -> Result<ExportSnapshot, CaptureError> {
        let mut query = QueryBuilder::<Postgres>::new(
            "SELECT coalesce(sum(c.stored_bytes), 0)::bigint AS total_bytes, \
             coalesce(max(c.seq), 0)::bigint AS last_seq FROM captures c",
        );
        push_selection(&mut query, endpoint, selection);
        let row = query
            .build()
            .fetch_one(self.pool())
            .await
            .map_err(classify)?;
        Ok(ExportSnapshot {
            total_bytes: row.try_get("total_bytes").map_err(CaptureError::Query)?,
            last_seq: row.try_get("last_seq").map_err(CaptureError::Query)?,
        })
    }

    pub async fn export_page(
        &self,
        endpoint: &EndpointId,
        selection: &ExportSelection,
        after_seq: i64,
        until_seq: i64,
        limit: i64,
    ) -> Result<Vec<CaptureDetail>, CaptureError> {
        let mut query = QueryBuilder::<Postgres>::new(format!(
            "SELECT {}, c.url, c.host, c.headers::text AS headers, c.query::text AS query, \
             c.form::text AS form, c.meta::text AS meta FROM captures c",
            summary_columns(None)
        ));
        push_selection(&mut query, endpoint, selection);
        query
            .push(" AND c.seq > ")
            .push_bind(after_seq)
            .push(" AND c.seq <= ")
            .push_bind(until_seq)
            .push(" ORDER BY c.seq ASC LIMIT ")
            .push_bind(limit);
        let rows = query
            .build()
            .fetch_all(self.pool())
            .await
            .map_err(classify)?;
        rows.iter().map(export_detail).collect()
    }
}

fn push_selection(
    query: &mut QueryBuilder<'_, Postgres>,
    endpoint: &EndpointId,
    selection: &ExportSelection,
) {
    match selection {
        ExportSelection::Filter(filter) => {
            push_filters(query, endpoint, filter);
            if let Some(after_seq) = filter.after_seq {
                query.push(" AND c.seq > ").push_bind(after_seq);
            }
        }
        ExportSelection::Ids(ids) => {
            query
                .push(" WHERE c.endpoint_id = ")
                .push_bind(endpoint.unpack().to_owned())
                .push(" AND c.id = ANY(")
                .push_bind(ids.clone())
                .push(")");
        }
    }
}

fn export_detail(row: &PgRow) -> Result<CaptureDetail, CaptureError> {
    let summary = summary_from_row(row)?;
    let context = summary.id.clone();
    let headers: String = row.try_get("headers").map_err(CaptureError::Query)?;
    let query: String = row.try_get("query").map_err(CaptureError::Query)?;
    let form: Option<String> = row.try_get("form").map_err(CaptureError::Query)?;
    let meta: String = row.try_get("meta").map_err(CaptureError::Query)?;
    let headers: Vec<NameValue> = decode(&context, "headers", &headers)?;
    let query: Vec<NameValue> = decode(&context, "query", &query)?;
    let form: Option<CaptureForm> = form
        .map(|text| decode(&context, "form", &text))
        .transpose()?;
    Ok(CaptureDetail {
        url: row.try_get("url").map_err(CaptureError::Query)?,
        host: row.try_get("host").map_err(CaptureError::Query)?,
        headers,
        query,
        form,
        meta: decode(&context, "meta", &meta)?,
        body: CaptureBody {
            prefix: Vec::new(),
            truncated: false,
        },
        summary,
    })
}
