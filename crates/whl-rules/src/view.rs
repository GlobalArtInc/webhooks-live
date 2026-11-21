use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;
use time::macros::format_description;
use uuid::Uuid;

use crate::log::{Entry, fit_entries};

pub const EVENT_PAYLOAD_LIMIT: usize = 7900;
const EVENT_TYPE: &str = "rule_result";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunView {
    pub id: Uuid,
    pub capture_id: Uuid,
    pub trigger: &'static str,
    pub state: &'static str,
    pub rules_version: i32,
    pub response_status: Option<u16>,
    pub error: Option<String>,
    pub entries: Vec<Entry>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

pub fn timestamp(moment: OffsetDateTime) -> String {
    moment
        .to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .unwrap_or_default()
}

fn encode(endpoint: &str, view: &RunView) -> Result<String, serde_json::Error> {
    serde_json::to_string(&json!({
        "t": EVENT_TYPE,
        "e": endpoint,
        "d": {"capture_id": view.capture_id, "run": view},
    }))
}

pub fn rule_result_payload(endpoint: &str, view: &RunView) -> Result<String, serde_json::Error> {
    let mut fitted = view.clone();
    fitted.entries = Vec::new();
    let overhead = encode(endpoint, &fitted)?.len();
    let room = EVENT_PAYLOAD_LIMIT.saturating_sub(overhead) + 2;
    fitted.entries = fit_entries(view.entries.clone(), room);
    encode(endpoint, &fitted)
}
