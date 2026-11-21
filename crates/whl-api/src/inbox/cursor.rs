use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use whl_core::endpoint_id::EndpointId;

const MAX_CURSOR_TEXT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanePosition {
    Top,
    Before(i64),
    Exhausted,
}

impl LanePosition {
    fn to_wire(self) -> Option<i64> {
        match self {
            Self::Top => None,
            Self::Before(seq) => Some(seq),
            Self::Exhausted => Some(0),
        }
    }

    fn from_wire(wire: Option<i64>) -> Option<Self> {
        match wire {
            None => Some(Self::Top),
            Some(0) => Some(Self::Exhausted),
            Some(seq) if seq > 0 => Some(Self::Before(seq)),
            Some(_) => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Wire {
    f: String,
    p: BTreeMap<String, Option<i64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxCursor {
    fingerprint: String,
    lanes: BTreeMap<String, LanePosition>,
}

impl InboxCursor {
    pub fn new(fingerprint: String, lanes: BTreeMap<String, LanePosition>) -> Self {
        Self { fingerprint, lanes }
    }

    pub fn lane(&self, endpoint: &EndpointId) -> Option<LanePosition> {
        self.lanes.get(endpoint.unpack()).copied()
    }

    pub fn endpoints(&self) -> Vec<EndpointId> {
        self.lanes
            .keys()
            .filter_map(|endpoint| EndpointId::parse(endpoint).ok())
            .collect()
    }

    pub fn matches_filters(&self, fingerprint: &str) -> bool {
        self.fingerprint == fingerprint
    }

    pub fn belongs_to(&self, fingerprint: &str, endpoints: &[EndpointId]) -> bool {
        self.fingerprint == fingerprint
            && self.lanes.len() == endpoints.len()
            && endpoints
                .iter()
                .all(|endpoint| self.lanes.contains_key(endpoint.unpack()))
    }

    pub fn is_exhausted(&self) -> bool {
        self.lanes
            .values()
            .all(|position| *position == LanePosition::Exhausted)
    }

    pub fn encode(&self) -> Option<String> {
        let wire = Wire {
            f: self.fingerprint.clone(),
            p: self
                .lanes
                .iter()
                .map(|(endpoint, position)| (endpoint.clone(), position.to_wire()))
                .collect(),
        };
        let text = serde_json::to_string(&wire).ok()?;
        Some(URL_SAFE_NO_PAD.encode(text))
    }

    pub fn decode(text: &str) -> Option<Self> {
        if text.len() > MAX_CURSOR_TEXT_BYTES {
            return None;
        }
        let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
        let wire: Wire = serde_json::from_slice(&bytes).ok()?;
        let mut lanes = BTreeMap::new();
        for (endpoint, position) in wire.p {
            EndpointId::parse(&endpoint).ok()?;
            lanes.insert(endpoint, LanePosition::from_wire(position)?);
        }
        Some(Self {
            fingerprint: wire.f,
            lanes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "0123456789abcdefghjkmnpq";
    const SECOND: &str = "zzzzzzzzzzzzzzzzzzzzzzzz";

    fn cursor() -> InboxCursor {
        InboxCursor::new(
            "{\"methods\":[]}".to_owned(),
            BTreeMap::from([
                (FIRST.to_owned(), LanePosition::Before(41)),
                (SECOND.to_owned(), LanePosition::Exhausted),
            ]),
        )
    }

    #[test]
    fn a_cursor_round_trips_every_lane_state() {
        let mut lanes = cursor().lanes;
        lanes.insert("aaaaaaaaaaaaaaaaaaaaaaaa".to_owned(), LanePosition::Top);
        let original = InboxCursor::new("f".to_owned(), lanes);
        let text = original.encode().expect("encodes");
        assert_eq!(InboxCursor::decode(&text), Some(original));
    }

    #[test]
    fn a_cursor_belongs_only_to_its_filters_and_its_exact_selection() {
        let held = cursor();
        let both = [
            EndpointId::parse(FIRST).expect("id"),
            EndpointId::parse(SECOND).expect("id"),
        ];
        assert!(held.belongs_to("{\"methods\":[]}", &both));
        assert!(!held.belongs_to("other", &both));
        assert!(!held.belongs_to("{\"methods\":[]}", &both[..1]));
        let swapped = [
            EndpointId::parse(FIRST).expect("id"),
            EndpointId::parse("aaaaaaaaaaaaaaaaaaaaaaaa").expect("id"),
        ];
        assert!(!held.belongs_to("{\"methods\":[]}", &swapped));
    }

    #[test]
    fn a_malformed_cursor_is_refused() {
        let bad_position = URL_SAFE_NO_PAD.encode(format!(r#"{{"f":"x","p":{{"{FIRST}":-3}}}}"#));
        let bad_id = URL_SAFE_NO_PAD.encode(r#"{"f":"x","p":{"short":1}}"#);
        for text in [
            "",
            "!!!",
            &URL_SAFE_NO_PAD.encode("not json"),
            &bad_position,
            &bad_id,
            &"A".repeat(MAX_CURSOR_TEXT_BYTES + 1),
        ] {
            assert_eq!(InboxCursor::decode(text), None, "{text:?}");
        }
    }
}
