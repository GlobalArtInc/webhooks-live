use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use whl_core::endpoint_id::EndpointId;

pub const MAX_VECTOR_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_VECTOR_ENTRIES: usize = 64;

const VERSION_PREFIX: &str = "1|";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VectorError {
    #[error("the vector text is longer than {MAX_VECTOR_TEXT_BYTES} bytes")]
    TooLong,
    #[error("the vector text is not base64url")]
    NotBase64,
    #[error("the vector is not UTF-8")]
    NotUtf8,
    #[error("the vector has an unknown version")]
    Version,
    #[error("the vector holds more than {MAX_VECTOR_ENTRIES} entries")]
    TooManyEntries,
    #[error("the vector holds a malformed entry")]
    Entry,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeqVector {
    positions: BTreeMap<String, i64>,
}

impl SeqVector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, endpoint: &EndpointId) -> Option<i64> {
        self.positions.get(endpoint.unpack()).copied()
    }

    pub fn set(&mut self, endpoint: &EndpointId, seq: i64) {
        self.positions.insert(endpoint.unpack().to_owned(), seq);
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn merge_max(&mut self, other: &Self) {
        for (endpoint, seq) in &other.positions {
            let slot = self.positions.entry(endpoint.clone()).or_insert(*seq);
            *slot = (*slot).max(*seq);
        }
    }

    pub fn encode(&self) -> String {
        let entries: Vec<String> = self
            .positions
            .iter()
            .map(|(endpoint, seq)| format!("{endpoint}:{seq}"))
            .collect();
        URL_SAFE_NO_PAD.encode(format!("{VERSION_PREFIX}{}", entries.join(",")))
    }

    pub fn decode(text: &str) -> Result<Self, VectorError> {
        if text.len() > MAX_VECTOR_TEXT_BYTES {
            return Err(VectorError::TooLong);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(text.trim())
            .map_err(|_| VectorError::NotBase64)?;
        let decoded = String::from_utf8(bytes).map_err(|_| VectorError::NotUtf8)?;
        let body = decoded
            .strip_prefix(VERSION_PREFIX)
            .ok_or(VectorError::Version)?;
        let mut vector = Self::new();
        if body.is_empty() {
            return Ok(vector);
        }
        for entry in body.split(',') {
            if vector.positions.len() >= MAX_VECTOR_ENTRIES {
                return Err(VectorError::TooManyEntries);
            }
            let (raw_endpoint, raw_seq) = entry.split_once(':').ok_or(VectorError::Entry)?;
            let endpoint = EndpointId::parse(raw_endpoint).map_err(|_| VectorError::Entry)?;
            let seq: i64 = raw_seq.parse().map_err(|_| VectorError::Entry)?;
            if seq < 0 {
                return Err(VectorError::Entry);
            }
            let slot = vector
                .positions
                .entry(endpoint.unpack().to_owned())
                .or_insert(seq);
            *slot = (*slot).max(seq);
        }
        Ok(vector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: &str = "0123456789abcdefghjkmnpq";
    const SECOND: &str = "zzzzzzzzzzzzzzzzzzzzzzzz";

    fn id(text: &str) -> EndpointId {
        EndpointId::parse(text).expect("a valid endpoint id")
    }

    #[test]
    fn a_vector_round_trips_in_a_stable_order() {
        let mut vector = SeqVector::new();
        vector.set(&id(SECOND), 9);
        vector.set(&id(FIRST), 4);
        let text = vector.encode();
        assert_eq!(SeqVector::decode(&text), Ok(vector.clone()));
        let mut reversed = SeqVector::new();
        reversed.set(&id(FIRST), 4);
        reversed.set(&id(SECOND), 9);
        assert_eq!(reversed.encode(), text);
    }

    #[test]
    fn an_empty_vector_round_trips() {
        let text = SeqVector::new().encode();
        assert_eq!(SeqVector::decode(&text), Ok(SeqVector::new()));
    }

    #[test]
    fn merging_keeps_the_larger_position_per_endpoint() {
        let mut left = SeqVector::new();
        left.set(&id(FIRST), 4);
        let mut right = SeqVector::new();
        right.set(&id(FIRST), 2);
        right.set(&id(SECOND), 7);
        left.merge_max(&right);
        assert_eq!(left.get(&id(FIRST)), Some(4));
        assert_eq!(left.get(&id(SECOND)), Some(7));
    }

    #[test]
    fn malformed_text_is_refused_with_its_reason() {
        let wrong_version = URL_SAFE_NO_PAD.encode("2|");
        let no_colon = URL_SAFE_NO_PAD.encode(format!("1|{FIRST}"));
        let bad_id = URL_SAFE_NO_PAD.encode("1|short:1");
        let negative = URL_SAFE_NO_PAD.encode(format!("1|{FIRST}:-1"));
        let not_number = URL_SAFE_NO_PAD.encode(format!("1|{FIRST}:x"));
        let too_many = URL_SAFE_NO_PAD.encode(format!(
            "1|{}",
            (0..=MAX_VECTOR_ENTRIES)
                .map(|index| format!("{index:024}:1").replace('0', "a"))
                .collect::<Vec<_>>()
                .join(",")
        ));
        assert_eq!(SeqVector::decode("!!"), Err(VectorError::NotBase64));
        assert_eq!(SeqVector::decode(&wrong_version), Err(VectorError::Version));
        assert_eq!(SeqVector::decode(&no_colon), Err(VectorError::Entry));
        assert_eq!(SeqVector::decode(&bad_id), Err(VectorError::Entry));
        assert_eq!(SeqVector::decode(&negative), Err(VectorError::Entry));
        assert_eq!(SeqVector::decode(&not_number), Err(VectorError::Entry));
        assert_eq!(
            SeqVector::decode(&"A".repeat(MAX_VECTOR_TEXT_BYTES + 1)),
            Err(VectorError::TooLong)
        );
        assert_eq!(
            SeqVector::decode(&too_many),
            Err(VectorError::TooManyEntries)
        );
    }
}
