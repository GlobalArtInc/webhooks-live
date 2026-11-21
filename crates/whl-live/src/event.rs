use serde::Deserialize;
use serde_json::Value;
use whl_core::endpoint_id::{EndpointId, EndpointIdError};

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("the payload is not a valid notification envelope: {0}")]
    Envelope(#[from] serde_json::Error),
    #[error("the notification names an invalid endpoint id: {0}")]
    EndpointId(#[from] EndpointIdError),
    #[error("the notification type {kind:?} is not known")]
    UnknownType { kind: String },
    #[error("a {kind} notification lacks the field {field}")]
    MissingField {
        kind: &'static str,
        field: &'static str,
    },
    #[error("a {kind} notification carries {field} that is not {expected}")]
    WrongShape {
        kind: &'static str,
        field: &'static str,
        expected: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResyncReason {
    Lagged,
    Gap,
    ListenerReconnect,
}

impl ResyncReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lagged => "lagged",
            Self::Gap => "gap",
            Self::ListenerReconnect => "listener_reconnect",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonText(String);

impl JsonText {
    pub fn unpack(&self) -> &str {
        &self.0
    }

    fn compact(value: &Value) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Capture { seq: i64, summary: JsonText },
    CaptureDeleted { data: JsonText },
    CapturesCleared,
    EndpointUpdated { data: JsonText },
    EndpointDeleted,
    ForwardResult { data: JsonText },
    Resync { reason: ResyncReason },
    Bye,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    t: String,
    e: String,
    seq: Option<Value>,
    s: Option<Value>,
    d: Option<Value>,
}

const EMPTY_OBJECT: &str = "{}";

impl Event {
    pub fn decode(payload: &str) -> Result<(EndpointId, Self), DecodeError> {
        let envelope: Envelope = serde_json::from_str(payload)?;
        let endpoint = EndpointId::parse(&envelope.e)?;
        let event = match envelope.t.as_str() {
            "capture" => capture(&envelope)?,
            "capture_deleted" => Self::CaptureDeleted {
                data: object(&envelope, "capture_deleted")?,
            },
            "captures_cleared" => Self::CapturesCleared,
            "endpoint_updated" => Self::EndpointUpdated {
                data: object(&envelope, "endpoint_updated")?,
            },
            "endpoint_deleted" => Self::EndpointDeleted,
            "forward_result" => Self::ForwardResult {
                data: object(&envelope, "forward_result")?,
            },
            _ => return Err(DecodeError::UnknownType { kind: envelope.t }),
        };
        Ok((endpoint, event))
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Capture { .. } => "capture",
            Self::CaptureDeleted { .. } => "capture_deleted",
            Self::CapturesCleared => "captures_cleared",
            Self::EndpointUpdated { .. } => "endpoint_updated",
            Self::EndpointDeleted => "endpoint_deleted",
            Self::ForwardResult { .. } => "forward_result",
            Self::Resync { .. } => "resync",
            Self::Bye => "bye",
        }
    }

    pub fn event_id(&self) -> Option<i64> {
        match self {
            Self::Capture { seq, .. } => Some(*seq),
            _ => None,
        }
    }

    pub fn data(&self) -> String {
        match self {
            Self::Capture { summary, .. } => summary.unpack().to_owned(),
            Self::CaptureDeleted { data }
            | Self::EndpointUpdated { data }
            | Self::ForwardResult { data } => data.unpack().to_owned(),
            Self::CapturesCleared | Self::EndpointDeleted | Self::Bye => EMPTY_OBJECT.to_owned(),
            Self::Resync { reason } => format!(r#"{{"reason":"{}"}}"#, reason.as_str()),
        }
    }
}

fn capture(envelope: &Envelope) -> Result<Event, DecodeError> {
    let seq = envelope
        .seq
        .as_ref()
        .ok_or(DecodeError::MissingField {
            kind: "capture",
            field: "seq",
        })?
        .as_i64()
        .filter(|seq| *seq >= 0)
        .ok_or(DecodeError::WrongShape {
            kind: "capture",
            field: "seq",
            expected: "a non-negative integer",
        })?;
    let summary = envelope.s.as_ref().ok_or(DecodeError::MissingField {
        kind: "capture",
        field: "s",
    })?;
    if !summary.is_object() {
        return Err(DecodeError::WrongShape {
            kind: "capture",
            field: "s",
            expected: "an object",
        });
    }
    Ok(Event::Capture {
        seq,
        summary: JsonText::compact(summary),
    })
}

fn object(envelope: &Envelope, kind: &'static str) -> Result<JsonText, DecodeError> {
    let data = envelope
        .d
        .as_ref()
        .ok_or(DecodeError::MissingField { kind, field: "d" })?;
    if !data.is_object() {
        return Err(DecodeError::WrongShape {
            kind,
            field: "d",
            expected: "an object",
        });
    }
    Ok(JsonText::compact(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdefghjkmnpq";

    fn decoded(payload: &str) -> Event {
        Event::decode(payload).expect("the payload decodes").1
    }

    #[test]
    fn decodes_a_capture_with_its_seq_and_compact_summary() {
        let event = decoded(&format!(
            r#"{{"t":"capture","e":"{ID}","seq":4,"s":{{ "a": 1,
 "b": "x" }}}}"#
        ));
        assert_eq!(event.event_id(), Some(4));
        assert_eq!(event.name(), "capture");
        assert_eq!(event.data(), r#"{"a":1,"b":"x"}"#);
    }

    #[test]
    fn decodes_the_other_types() {
        let with_data = |kind: &str| format!(r#"{{"t":"{kind}","e":"{ID}","d":{{"k":1}}}}"#);
        let without_data = |kind: &str| format!(r#"{{"t":"{kind}","e":"{ID}"}}"#);
        assert_eq!(
            decoded(&with_data("capture_deleted")).name(),
            "capture_deleted"
        );
        assert_eq!(decoded(&with_data("endpoint_updated")).data(), r#"{"k":1}"#);
        assert_eq!(
            decoded(&with_data("forward_result")).name(),
            "forward_result"
        );
        assert_eq!(
            decoded(&without_data("captures_cleared")),
            Event::CapturesCleared
        );
        assert_eq!(
            decoded(&without_data("endpoint_deleted")),
            Event::EndpointDeleted
        );
    }

    #[test]
    fn refuses_what_is_not_a_well_formed_notification() {
        let refused = [
            "".to_owned(),
            "[]".to_owned(),
            r#"{"t":"capture","e":"bad","seq":1,"s":{}}"#.to_owned(),
            format!(r#"{{"t":"capture","e":"{ID}","s":{{}}}}"#),
            format!(r#"{{"t":"capture","e":"{ID}","seq":-1,"s":{{}}}}"#),
            format!(r#"{{"t":"capture","e":"{ID}","seq":1,"s":"text"}}"#),
            format!(r#"{{"t":"capture_deleted","e":"{ID}"}}"#),
            format!(r#"{{"t":"forward_result","e":"{ID}","d":1}}"#),
            format!(r#"{{"t":"resync","e":"{ID}"}}"#),
            format!(r#"{{"t":"bye","e":"{ID}"}}"#),
        ];
        for payload in refused {
            assert!(Event::decode(&payload).is_err(), "{payload}");
        }
        assert!(matches!(
            Event::decode(&format!(r#"{{"t":"new_thing","e":"{ID}"}}"#)),
            Err(DecodeError::UnknownType { .. })
        ));
    }

    #[test]
    fn renders_resync_and_bye() {
        let resync = Event::Resync {
            reason: ResyncReason::ListenerReconnect,
        };
        assert_eq!(resync.data(), r#"{"reason":"listener_reconnect"}"#);
        assert_eq!(Event::Bye.data(), "{}");
        assert_eq!(Event::Bye.event_id(), None);
    }
}
