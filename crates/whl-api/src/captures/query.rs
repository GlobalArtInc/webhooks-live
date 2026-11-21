use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use whl_store::{CaptureFilter, CaptureKind, CaptureQuery, Order, StatusFilter};

use super::cursor::Cursor;
use crate::error::{ApiError, FieldCode, FieldViolation};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;
const MAX_SEARCH_CHARS: usize = 200;
const INBOX_DEFAULT_LIMIT: i64 = 25;
const INBOX_MAX_LIMIT: i64 = 50;
const INBOX_SEARCH_CHARS: i32 = 4096;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListParams {
    order: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
    method: Option<String>,
    status: Option<String>,
    kind: Option<String>,
    starred: Option<String>,
    q: Option<String>,
    after_seq: Option<String>,
    since: Option<String>,
    until: Option<String>,
    endpoints: Option<String>,
    folder: Option<String>,
}

impl ListParams {
    pub fn endpoints(&self) -> Option<&str> {
        self.endpoints.as_deref()
    }

    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InboxQuery {
    pub filter: CaptureFilter,
    pub limit: i64,
    pub cursor: Option<String>,
}

pub fn inbox_query(params: &ListParams) -> Result<InboxQuery, ApiError> {
    let mut violations = Vec::new();
    if params.order.as_deref().is_some_and(|order| order != "desc") {
        violations.push(violation(
            "order",
            FieldCode::NotAllowed,
            "the combined inbox lists newest first only",
        ));
    }
    if params.after_seq.is_some() {
        violations.push(violation(
            "after_seq",
            FieldCode::NotAllowed,
            "after_seq names one endpoint's seq; list that endpoint instead",
        ));
    }
    let limit = parse_inbox_limit(params.limit.as_deref(), &mut violations);
    let filter = CaptureFilter {
        methods: parse_methods(params.method.as_deref(), &mut violations),
        statuses: parse_statuses(params.status.as_deref(), &mut violations),
        kinds: parse_kinds(params.kind.as_deref(), &mut violations),
        starred: None,
        search: parse_search(params.q.as_deref(), &mut violations),
        search_chars: Some(INBOX_SEARCH_CHARS),
        after_seq: None,
        since: parse_timestamp("since", params.since.as_deref(), &mut violations),
        until: parse_timestamp("until", params.until.as_deref(), &mut violations),
    };
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    Ok(InboxQuery {
        filter,
        limit,
        cursor: params.cursor.clone(),
    })
}

pub fn list_query(params: &ListParams) -> Result<CaptureQuery, ApiError> {
    let mut violations = Vec::new();
    let order = parse_order(params.order.as_deref(), &mut violations).unwrap_or(Order::Desc);
    let limit = parse_limit(params.limit.as_deref(), &mut violations).unwrap_or(DEFAULT_LIMIT);
    let filter = parse_filter(params, &mut violations);
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    let position = match params.cursor.as_deref() {
        Some(text) => {
            let cursor = Cursor::decode(text).ok_or(ApiError::InvalidCursor)?;
            if cursor.order != order || cursor.fingerprint != filter.fingerprint() {
                return Err(ApiError::InvalidCursor);
            }
            Some(cursor.position)
        }
        None => None,
    };
    Ok(CaptureQuery {
        filter,
        order,
        limit,
        position,
    })
}

pub(crate) fn export_filter(params: &ListParams) -> Result<CaptureFilter, ApiError> {
    let mut violations = Vec::new();
    let filter = parse_filter(params, &mut violations);
    if violations.is_empty() {
        Ok(filter)
    } else {
        Err(ApiError::Validation(violations))
    }
}

fn parse_filter(params: &ListParams, violations: &mut Vec<FieldViolation>) -> CaptureFilter {
    CaptureFilter {
        methods: parse_methods(params.method.as_deref(), violations),
        statuses: parse_statuses(params.status.as_deref(), violations),
        kinds: parse_kinds(params.kind.as_deref(), violations),
        starred: parse_starred(params.starred.as_deref(), violations),
        search: parse_search(params.q.as_deref(), violations),
        search_chars: None,
        after_seq: parse_after_seq(params.after_seq.as_deref(), violations),
        since: parse_timestamp("since", params.since.as_deref(), violations),
        until: parse_timestamp("until", params.until.as_deref(), violations),
    }
}

fn violation(field: &str, code: FieldCode, message: &str) -> FieldViolation {
    FieldViolation::new(field, code, message)
}

fn parse_order(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Option<Order> {
    match text {
        None | Some("desc") => Some(Order::Desc),
        Some("asc") => Some(Order::Asc),
        Some(_) => {
            violations.push(violation(
                "order",
                FieldCode::InvalidFormat,
                "must be asc or desc",
            ));
            None
        }
    }
}

fn parse_limit(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Option<i64> {
    let Some(text) = text else {
        return Some(DEFAULT_LIMIT);
    };
    match text.parse::<i64>() {
        Ok(limit) if (1..=MAX_LIMIT).contains(&limit) => Some(limit),
        Ok(_) => {
            violations.push(violation(
                "limit",
                FieldCode::OutOfRange,
                "must be between 1 and 200",
            ));
            None
        }
        Err(_) => {
            violations.push(violation(
                "limit",
                FieldCode::InvalidFormat,
                "must be an integer",
            ));
            None
        }
    }
}

fn parse_inbox_limit(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> i64 {
    let Some(text) = text else {
        return INBOX_DEFAULT_LIMIT;
    };
    match text.parse::<i64>() {
        Ok(limit) if (1..=INBOX_MAX_LIMIT).contains(&limit) => limit,
        Ok(_) => {
            violations.push(violation(
                "limit",
                FieldCode::OutOfRange,
                "must be between 1 and 50",
            ));
            INBOX_DEFAULT_LIMIT
        }
        Err(_) => {
            violations.push(violation(
                "limit",
                FieldCode::InvalidFormat,
                "must be an integer",
            ));
            INBOX_DEFAULT_LIMIT
        }
    }
}

fn is_token_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(character)
}

fn parse_methods(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Vec<String> {
    let Some(text) = text else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    for token in text.split(',') {
        if token.is_empty() || !token.chars().all(is_token_char) {
            violations.push(violation(
                "method",
                FieldCode::InvalidFormat,
                "must be a comma list of HTTP method tokens",
            ));
            return Vec::new();
        }
        methods.push(token.to_owned());
    }
    methods.sort();
    methods.dedup();
    methods
}

fn parse_statuses(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Vec<StatusFilter> {
    let Some(text) = text else {
        return Vec::new();
    };
    let mut statuses: Vec<StatusFilter> = Vec::new();
    for token in text.split(',') {
        let status = match token {
            "none" => Some(StatusFilter::Unoverridden),
            _ => class_of(token).map(StatusFilter::Class),
        };
        match status {
            Some(status) => {
                if !statuses.contains(&status) {
                    statuses.push(status);
                }
            }
            None => {
                violations.push(violation(
                    "status",
                    FieldCode::InvalidFormat,
                    "must be a comma list of 1xx to 5xx or none",
                ));
                return Vec::new();
            }
        }
    }
    statuses.sort_by_key(|status| match status {
        StatusFilter::Unoverridden => 0,
        StatusFilter::Class(class) => *class,
    });
    statuses
}

fn parse_kinds(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Vec<CaptureKind> {
    let Some(text) = text else {
        return Vec::new();
    };
    let mut kinds = Vec::new();
    for token in text.split(',') {
        match CaptureKind::parse(token) {
            Some(kind) => kinds.push(kind),
            None => {
                violations.push(violation(
                    "kind",
                    FieldCode::InvalidFormat,
                    "must be a comma list of http, email, dns or schedule",
                ));
                return Vec::new();
            }
        }
    }
    kinds.sort_by_key(|kind| kind.as_str());
    kinds.dedup();
    kinds
}

fn parse_starred(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Option<bool> {
    match text? {
        "true" => Some(true),
        "false" => Some(false),
        _ => {
            violations.push(violation(
                "starred",
                FieldCode::InvalidFormat,
                "must be true or false",
            ));
            None
        }
    }
}

fn class_of(token: &str) -> Option<u8> {
    if token.len() != 3 || !token.ends_with("xx") {
        return None;
    }
    let digit = token.as_bytes()[0];
    (b'1'..=b'5').contains(&digit).then(|| digit - b'0')
}

fn parse_search(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Option<String> {
    let text = text?;
    let length = text.chars().count();
    if length == 0 {
        violations.push(violation("q", FieldCode::Required, "must not be empty"));
        None
    } else if length > MAX_SEARCH_CHARS {
        violations.push(violation(
            "q",
            FieldCode::TooLong,
            "must be at most 200 characters",
        ));
        None
    } else {
        Some(text.to_owned())
    }
}

fn parse_after_seq(text: Option<&str>, violations: &mut Vec<FieldViolation>) -> Option<i64> {
    let text = text?;
    match text.parse::<i64>() {
        Ok(seq) if seq >= 0 => Some(seq),
        Ok(_) => {
            violations.push(violation(
                "after_seq",
                FieldCode::OutOfRange,
                "must be zero or greater",
            ));
            None
        }
        Err(_) => {
            violations.push(violation(
                "after_seq",
                FieldCode::InvalidFormat,
                "must be an integer",
            ));
            None
        }
    }
}

fn parse_timestamp(
    field: &str,
    text: Option<&str>,
    violations: &mut Vec<FieldViolation>,
) -> Option<OffsetDateTime> {
    let text = text?;
    match OffsetDateTime::parse(text, &Rfc3339) {
        Ok(timestamp) => Some(timestamp),
        Err(_) => {
            violations.push(violation(
                field,
                FieldCode::InvalidFormat,
                "must be an RFC 3339 timestamp",
            ));
            None
        }
    }
}
