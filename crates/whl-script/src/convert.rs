use std::io::Write;

use rhai::{Array, Dynamic, ImmutableString, Map};
use serde_json::{Number, Value};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum ConvertError {
    #[error("nesting is deeper than {0} levels")]
    TooDeep(usize),
    #[error("more than {0} values")]
    TooManyNodes(usize),
    #[error("more than {0} bytes of text")]
    TooManyBytes(usize),
    #[error("a string is longer than {0} bytes")]
    StringTooLarge(usize),
    #[error("arrays hold more than {0} items in total")]
    ArrayTooLarge(usize),
    #[error("maps hold more than {0} entries in total")]
    MapTooLarge(usize),
    #[error("a map key is longer than {0} bytes")]
    KeyTooLarge(usize),
    #[error("a number is not finite")]
    NotFinite,
    #[error("a value of type {0} cannot be represented as JSON")]
    Unsupported(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConvertLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
    pub max_string_bytes: usize,
    pub max_array_len: usize,
    pub max_map_len: usize,
    pub max_key_bytes: usize,
}

struct Walker {
    limits: ConvertLimits,
    nodes: usize,
    bytes: usize,
    array_items: usize,
    map_entries: usize,
}

impl Walker {
    fn new(limits: ConvertLimits) -> Self {
        Self {
            limits,
            nodes: 0,
            bytes: 0,
            array_items: 0,
            map_entries: 0,
        }
    }

    fn array_items(&mut self, count: usize) -> Result<(), ConvertError> {
        self.array_items = self.array_items.saturating_add(count);
        if self.array_items > self.limits.max_array_len {
            return Err(ConvertError::ArrayTooLarge(self.limits.max_array_len));
        }
        Ok(())
    }

    fn map_entries(&mut self, count: usize) -> Result<(), ConvertError> {
        self.map_entries = self.map_entries.saturating_add(count);
        if self.map_entries > self.limits.max_map_len {
            return Err(ConvertError::MapTooLarge(self.limits.max_map_len));
        }
        Ok(())
    }

    fn node(&mut self, depth: usize) -> Result<(), ConvertError> {
        if depth > self.limits.max_depth {
            return Err(ConvertError::TooDeep(self.limits.max_depth));
        }
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            return Err(ConvertError::TooManyNodes(self.limits.max_nodes));
        }
        Ok(())
    }

    fn text(&mut self, length: usize, key: bool) -> Result<(), ConvertError> {
        if key && length > self.limits.max_key_bytes {
            return Err(ConvertError::KeyTooLarge(self.limits.max_key_bytes));
        }
        if !key && length > self.limits.max_string_bytes {
            return Err(ConvertError::StringTooLarge(self.limits.max_string_bytes));
        }
        self.bytes = self.bytes.saturating_add(length);
        if self.bytes > self.limits.max_bytes {
            return Err(ConvertError::TooManyBytes(self.limits.max_bytes));
        }
        Ok(())
    }

    fn json_to_dynamic(&mut self, value: &Value, depth: usize) -> Result<Dynamic, ConvertError> {
        self.node(depth)?;
        Ok(match value {
            Value::Null => Dynamic::UNIT,
            Value::Bool(flag) => Dynamic::from(*flag),
            Value::Number(number) => number_to_dynamic(number),
            Value::String(text) => {
                self.text(text.len(), false)?;
                Dynamic::from(text.clone())
            }
            Value::Array(items) => {
                self.array_items(items.len())?;
                let mut array = Array::with_capacity(items.len());
                for item in items {
                    array.push(self.json_to_dynamic(item, depth + 1)?);
                }
                Dynamic::from_array(array)
            }
            Value::Object(entries) => self.object_to_dynamic(entries, depth)?,
        })
    }

    fn object_to_dynamic(
        &mut self,
        entries: &serde_json::Map<String, Value>,
        depth: usize,
    ) -> Result<Dynamic, ConvertError> {
        self.map_entries(entries.len())?;
        let mut map = Map::new();
        for (key, item) in entries {
            self.text(key.len(), true)?;
            map.insert(key.as_str().into(), self.json_to_dynamic(item, depth + 1)?);
        }
        Ok(Dynamic::from_map(map))
    }

    fn dynamic_to_json(&mut self, value: &Dynamic, depth: usize) -> Result<Value, ConvertError> {
        self.node(depth)?;
        if value.is_unit() {
            return Ok(Value::Null);
        }
        if let Ok(flag) = value.as_bool() {
            return Ok(Value::Bool(flag));
        }
        if let Ok(number) = value.as_int() {
            return Ok(Value::Number(Number::from(number)));
        }
        if let Ok(number) = value.as_float() {
            return Number::from_f64(number)
                .map(Value::Number)
                .ok_or(ConvertError::NotFinite);
        }
        if let Ok(character) = value.as_char() {
            let text = character.to_string();
            self.text(text.len(), false)?;
            return Ok(Value::String(text));
        }
        if let Some(text) = value.read_lock::<ImmutableString>() {
            self.text(text.len(), false)?;
            return Ok(Value::String(text.as_str().to_owned()));
        }
        if let Some(array) = value.read_lock::<Array>() {
            self.array_items(array.len())?;
            let mut items = Vec::with_capacity(array.len());
            for item in array.iter() {
                items.push(self.dynamic_to_json(item, depth + 1)?);
            }
            return Ok(Value::Array(items));
        }
        if let Some(map) = value.read_lock::<Map>() {
            self.map_entries(map.len())?;
            let mut entries = serde_json::Map::new();
            for (key, item) in map.iter() {
                self.text(key.len(), true)?;
                entries.insert(key.to_string(), self.dynamic_to_json(item, depth + 1)?);
            }
            return Ok(Value::Object(entries));
        }
        Err(ConvertError::Unsupported(type_label(value)))
    }
}

fn type_label(value: &Dynamic) -> &'static str {
    if value.is::<rhai::FnPtr>() {
        "function pointer"
    } else if value.is::<std::ops::Range<i64>>() || value.is::<std::ops::RangeInclusive<i64>>() {
        "range"
    } else {
        "unsupported"
    }
}

fn number_to_dynamic(number: &Number) -> Dynamic {
    if let Some(integer) = number.as_i64() {
        Dynamic::from(integer)
    } else {
        Dynamic::from(number.as_f64().unwrap_or(f64::NAN))
    }
}

pub(crate) fn json_to_dynamic(
    value: &Value,
    limits: ConvertLimits,
) -> Result<Dynamic, ConvertError> {
    Walker::new(limits).json_to_dynamic(value, 0)
}

pub(crate) fn object_to_dynamic(
    entries: &serde_json::Map<String, Value>,
    limits: ConvertLimits,
) -> Result<Dynamic, ConvertError> {
    let mut walker = Walker::new(limits);
    walker.node(0)?;
    walker.object_to_dynamic(entries, 0)
}

pub(crate) fn dynamic_to_json(
    value: &Dynamic,
    limits: ConvertLimits,
) -> Result<Value, ConvertError> {
    Walker::new(limits).dynamic_to_json(value, 0)
}

pub(crate) struct BoundedWriter {
    buffer: Vec<u8>,
    limit: usize,
}

impl BoundedWriter {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit,
        }
    }

    pub(crate) fn into_string(self) -> Option<String> {
        String::from_utf8(self.buffer).ok()
    }
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.buffer.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "output limit reached",
            ));
        }
        self.buffer.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
