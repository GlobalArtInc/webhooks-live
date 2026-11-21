use std::borrow::Cow;

use serde_json::Value as JsonValue;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone)]
pub(crate) enum Value<'a> {
    Missing,
    Text(Cow<'a, str>),
    Int(i64),
    Time(OffsetDateTime),
}

impl<'a> Value<'a> {
    pub(crate) fn borrowed(text: &'a str) -> Self {
        Self::Text(Cow::Borrowed(text))
    }

    pub(crate) fn owned(text: String) -> Self {
        Self::Text(Cow::Owned(text))
    }

    pub(crate) fn is_blank(&self) -> bool {
        match self {
            Self::Missing => true,
            Self::Text(text) => text.is_empty(),
            Self::Int(_) | Self::Time(_) => false,
        }
    }

    pub(crate) fn into_owned(self) -> Value<'static> {
        match self {
            Self::Missing => Value::Missing,
            Self::Text(text) => Value::Text(Cow::Owned(text.into_owned())),
            Self::Int(number) => Value::Int(number),
            Self::Time(instant) => Value::Time(instant),
        }
    }

    pub(crate) fn into_text(self) -> Option<Cow<'a, str>> {
        match self {
            Self::Missing => Some(Cow::Borrowed("")),
            Self::Text(text) => Some(text),
            Self::Int(number) => Some(Cow::Owned(number.to_string())),
            Self::Time(instant) => instant
                .to_offset(time::UtcOffset::UTC)
                .format(&Rfc3339)
                .ok()
                .map(Cow::Owned),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JsonConversion {
    TooLarge,
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    max: usize,
}

impl std::io::Write for LimitedBuffer {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(data.len()) > self.max {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn compact_json<T: serde::Serialize>(
    value: &T,
    max: usize,
) -> Result<String, JsonConversion> {
    let mut buffer = LimitedBuffer {
        bytes: Vec::new(),
        max,
    };
    serde_json::to_writer(&mut buffer, value).map_err(|_| JsonConversion::TooLarge)?;
    Ok(String::from_utf8(buffer.bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned()))
}

pub(crate) fn json_to_value(value: &JsonValue, max: usize) -> Result<Value<'_>, JsonConversion> {
    match value {
        JsonValue::Null => Ok(Value::Missing),
        JsonValue::String(text) => Ok(Value::borrowed(text)),
        JsonValue::Bool(true) => Ok(Value::borrowed("true")),
        JsonValue::Bool(false) => Ok(Value::borrowed("false")),
        JsonValue::Number(number) => Ok(Value::owned(number.to_string())),
        JsonValue::Array(_) | JsonValue::Object(_) => compact_json(value, max).map(Value::owned),
    }
}
