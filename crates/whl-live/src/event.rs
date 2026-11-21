use std::future::Future;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Uuid;
use tokio::sync::OnceCell;
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

    pub(crate) fn compact(value: &Value) -> Self {
        Self(value.to_string())
    }

    pub(crate) fn from_serialized(text: String) -> Self {
        Self(text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Data(JsonText),
    Gone,
    Failed,
}

#[derive(Debug, Default)]
pub struct Lookup {
    outcome: OnceCell<Outcome>,
}

impl Lookup {
    pub async fn resolve<F>(&self, load: F) -> &Outcome
    where
        F: Future<Output = Outcome>,
    {
        self.outcome.get_or_init(|| load).await
    }

    pub fn resolved(&self) -> Option<&Outcome> {
        self.outcome.get()
    }
}

impl PartialEq for Lookup {
    fn eq(&self, other: &Self) -> bool {
        self.resolved() == other.resolved()
    }
}

impl Eq for Lookup {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Capture {
        seq: i64,
        summary: JsonText,
    },
    CaptureDeleted {
        data: JsonText,
    },
    CapturesCleared,
    EndpointUpdated {
        lookup: Arc<Lookup>,
    },
    EndpointDeleted,
    ForwardResult {
        capture_id: Uuid,
        job_id: Uuid,
        lookup: Arc<Lookup>,
    },
    Resync {
        reason: ResyncReason,
    },
    AccessChanged,
    Bye,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    Heartbeat,
    Event { endpoint: EndpointId, event: Event },
}

#[derive(Debug, Deserialize)]
struct Envelope {
    t: String,
    e: Option<String>,
    seq: Option<Value>,
    s: Option<Value>,
    d: Option<Value>,
}

const EMPTY_OBJECT: &str = "{}";
const HEARTBEAT: &str = "heartbeat";

impl Notification {
    pub fn decode(payload: &str) -> Result<Self, DecodeError> {
        let envelope: Envelope = serde_json::from_str(payload)?;
        if envelope.t == HEARTBEAT {
            return Ok(Self::Heartbeat);
        }
        let raw_endpoint = envelope.e.as_deref().ok_or(DecodeError::MissingField {
            kind: "notification",
            field: "e",
        })?;
        let endpoint = EndpointId::parse(raw_endpoint)?;
        let event = match envelope.t.as_str() {
            "capture" => capture(&envelope)?,
            "capture_deleted" => Event::CaptureDeleted {
                data: object(&envelope, "capture_deleted")?,
            },
            "captures_cleared" => Event::CapturesCleared,
            "endpoint_updated" => Event::EndpointUpdated {
                lookup: Arc::default(),
            },
            "endpoint_deleted" => Event::EndpointDeleted,
            "access_changed" => Event::AccessChanged,
            "forward_result" => forward_result(&envelope)?,
            _ => return Err(DecodeError::UnknownType { kind: envelope.t }),
        };
        Ok(Self::Event { endpoint, event })
    }
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Capture { .. } => "capture",
            Self::CaptureDeleted { .. } => "capture_deleted",
            Self::CapturesCleared => "captures_cleared",
            Self::EndpointUpdated { .. } => "endpoint_updated",
            Self::EndpointDeleted => "endpoint_deleted",
            Self::ForwardResult { .. } => "forward_result",
            Self::Resync { .. } => "resync",
            Self::AccessChanged => "access_changed",
            Self::Bye => "bye",
        }
    }

    pub fn event_id(&self) -> Option<i64> {
        match self {
            Self::Capture { seq, .. } => Some(*seq),
            _ => None,
        }
    }

    pub fn data(&self) -> Option<String> {
        match self {
            Self::Capture { summary, .. } => Some(summary.unpack().to_owned()),
            Self::CaptureDeleted { data } => Some(data.unpack().to_owned()),
            Self::EndpointUpdated { lookup } | Self::ForwardResult { lookup, .. } => {
                match lookup.resolved() {
                    Some(Outcome::Data(data)) => Some(data.unpack().to_owned()),
                    _ => None,
                }
            }
            Self::CapturesCleared | Self::EndpointDeleted | Self::Bye => {
                Some(EMPTY_OBJECT.to_owned())
            }
            Self::AccessChanged => None,
            Self::Resync { reason } => Some(format!(r#"{{"reason":"{}"}}"#, reason.as_str())),
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

fn forward_result(envelope: &Envelope) -> Result<Event, DecodeError> {
    const KIND: &str = "forward_result";
    let data = envelope.d.as_ref().ok_or(DecodeError::MissingField {
        kind: KIND,
        field: "d",
    })?;
    let uuid = |field: &'static str| -> Result<Uuid, DecodeError> {
        data.get(field)
            .ok_or(DecodeError::MissingField { kind: KIND, field })?
            .as_str()
            .and_then(|text| Uuid::try_parse(text).ok())
            .ok_or(DecodeError::WrongShape {
                kind: KIND,
                field,
                expected: "a UUID",
            })
    };
    Ok(Event::ForwardResult {
        capture_id: uuid("capture_id")?,
        job_id: uuid("job_id")?,
        lookup: Arc::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdefghjkmnpq";
    const CAPTURE: &str = "0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b";
    const JOB: &str = "0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6c";

    fn decoded(payload: &str) -> Event {
        match Notification::decode(payload).expect("the payload decodes") {
            Notification::Event { event, .. } => event,
            Notification::Heartbeat => panic!("a heartbeat is not an event"),
        }
    }

    #[test]
    fn decodes_a_capture_with_its_seq_and_compact_summary() {
        let event = decoded(&format!(
            r#"{{"t":"capture","e":"{ID}","seq":4,"s":{{ "a": 1,
 "b": "x" }}}}"#
        ));
        assert_eq!(event.event_id(), Some(4));
        assert_eq!(event.name(), "capture");
        assert_eq!(event.data().as_deref(), Some(r#"{"a":1,"b":"x"}"#));
    }

    #[test]
    fn decodes_the_other_types() {
        let without_data = |kind: &str| format!(r#"{{"t":"{kind}","e":"{ID}"}}"#);
        let deleted =
            format!(r#"{{"t":"capture_deleted","e":"{ID}","d":{{"capture_id":"c","seq":1}}}}"#);
        assert_eq!(decoded(&deleted).name(), "capture_deleted");
        assert_eq!(
            decoded(&deleted).data().as_deref(),
            Some(r#"{"capture_id":"c","seq":1}"#)
        );
        assert_eq!(
            decoded(&without_data("captures_cleared")),
            Event::CapturesCleared
        );
        assert_eq!(
            decoded(&without_data("endpoint_deleted")),
            Event::EndpointDeleted
        );
        assert_eq!(
            decoded(&without_data("access_changed")),
            Event::AccessChanged
        );
    }

    #[test]
    fn thin_events_carry_ids_and_no_data_until_resolved() {
        let updated = decoded(&format!(r#"{{"t":"endpoint_updated","e":"{ID}"}}"#));
        assert_eq!(updated.name(), "endpoint_updated");
        assert_eq!(updated.data(), None);

        let result = decoded(&format!(
            r#"{{"t":"forward_result","e":"{ID}","d":{{"capture_id":"{CAPTURE}","job_id":"{JOB}"}}}}"#
        ));
        let Event::ForwardResult {
            capture_id, job_id, ..
        } = &result
        else {
            panic!("expected a forward result");
        };
        assert_eq!(capture_id.to_string(), CAPTURE);
        assert_eq!(job_id.to_string(), JOB);
        assert_eq!(result.data(), None);
    }

    #[test]
    fn decodes_a_heartbeat_without_an_endpoint() {
        assert_eq!(
            Notification::decode(r#"{"t":"heartbeat"}"#).expect("the heartbeat decodes"),
            Notification::Heartbeat
        );
    }

    #[test]
    fn refuses_what_is_not_a_well_formed_notification() {
        let refused = [
            "".to_owned(),
            "[]".to_owned(),
            r#"{"t":"capture","e":"bad","seq":1,"s":{}}"#.to_owned(),
            r#"{"t":"capture","seq":1,"s":{}}"#.to_owned(),
            format!(r#"{{"t":"capture","e":"{ID}","s":{{}}}}"#),
            format!(r#"{{"t":"capture","e":"{ID}","seq":-1,"s":{{}}}}"#),
            format!(r#"{{"t":"capture","e":"{ID}","seq":1,"s":"text"}}"#),
            format!(r#"{{"t":"capture_deleted","e":"{ID}"}}"#),
            format!(r#"{{"t":"forward_result","e":"{ID}"}}"#),
            format!(r#"{{"t":"forward_result","e":"{ID}","d":{{"capture_id":"{CAPTURE}"}}}}"#),
            format!(
                r#"{{"t":"forward_result","e":"{ID}","d":{{"capture_id":"x","job_id":"{JOB}"}}}}"#
            ),
            format!(r#"{{"t":"resync","e":"{ID}"}}"#),
            format!(r#"{{"t":"bye","e":"{ID}"}}"#),
        ];
        for payload in refused {
            assert!(Notification::decode(&payload).is_err(), "{payload}");
        }
        assert!(matches!(
            Notification::decode(&format!(r#"{{"t":"new_thing","e":"{ID}"}}"#)),
            Err(DecodeError::UnknownType { .. })
        ));
    }

    #[test]
    fn renders_resync_and_bye() {
        let resync = Event::Resync {
            reason: ResyncReason::ListenerReconnect,
        };
        assert_eq!(
            resync.data().as_deref(),
            Some(r#"{"reason":"listener_reconnect"}"#)
        );
        assert_eq!(Event::Bye.data().as_deref(), Some("{}"));
        assert_eq!(Event::Bye.event_id(), None);
    }

    #[tokio::test]
    async fn a_lookup_loads_once_for_concurrent_resolvers() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let lookup = Lookup::default();
        let loads = AtomicUsize::new(0);
        let load = || async {
            loads.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Outcome::Data(JsonText::from_serialized("{}".to_owned()))
        };

        let (first, second, third) = tokio::join!(
            lookup.resolve(load()),
            lookup.resolve(load()),
            lookup.resolve(load())
        );

        assert_eq!(loads.load(Ordering::SeqCst), 1);
        assert_eq!(first, second);
        assert_eq!(second, third);
    }
}
