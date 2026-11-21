use std::fmt;
use std::io::{self, Write};

use serde_json::{Map, Number, Value};
use time::OffsetDateTime;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;

use crate::TelemetryError;

type Fields = Map<String, Value>;

const TIMESTAMP_FORMAT: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

struct SpanFields {
    values: Fields,
}

pub fn init(filter: &str) -> Result<(), TelemetryError> {
    let filter = EnvFilter::try_new(filter)?;
    tracing_subscriber::registry()
        .with(filter)
        .with(JsonLines)
        .try_init()?;
    Ok(())
}

fn timestamp(moment: OffsetDateTime) -> Value {
    moment
        .format(TIMESTAMP_FORMAT)
        .map_or(Value::Null, Value::from)
}

struct JsonLines;

impl<S> Layer<S> for JsonLines
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        let mut values = Fields::new();
        attributes.record(&mut FieldVisitor(&mut values));
        span.extensions_mut().insert(SpanFields { values });
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, context: Context<'_, S>) {
        let Some(span) = context.span(id) else {
            return;
        };
        if let Some(fields) = span.extensions_mut().get_mut::<SpanFields>() {
            values.record(&mut FieldVisitor(&mut fields.values));
        }
    }

    fn on_event(&self, event: &Event<'_>, context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut line = Fields::new();
        line.insert("ts".to_owned(), timestamp(OffsetDateTime::now_utc()));
        line.insert(
            "level".to_owned(),
            Value::from(metadata.level().to_string()),
        );
        line.insert("target".to_owned(), Value::from(metadata.target()));

        if let Some(scope) = context.event_scope(event) {
            for span in scope.from_root() {
                if let Some(fields) = span.extensions().get::<SpanFields>() {
                    line.extend(fields.values.clone());
                }
            }
        }
        event.record(&mut FieldVisitor(&mut line));

        let mut stdout = io::stdout().lock();
        if serde_json::to_writer(&mut stdout, &line).is_ok() {
            let _ = stdout.write_all(b"\n");
        }
    }
}

struct FieldVisitor<'a>(&'a mut Fields);

impl FieldVisitor<'_> {
    fn put(&mut self, field: &Field, value: Value) {
        let key = match field.name() {
            "message" => "msg",
            name => name,
        };
        self.0.insert(key.to_owned(), value);
    }
}

impl Visit for FieldVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field, Value::from(format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::from(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(
            field,
            Number::from_f64(value).map_or(Value::Null, Value::Number),
        );
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use time::OffsetDateTime;

    use super::timestamp;

    #[test]
    fn timestamp_is_rfc3339_utc_with_millisecond_precision() {
        let moment = OffsetDateTime::from_unix_timestamp_nanos(1_795_264_496_007_654_321)
            .expect("timestamp is in range");

        assert_eq!(timestamp(moment), Value::from("2026-11-21T12:34:56.007Z"));
    }
}
