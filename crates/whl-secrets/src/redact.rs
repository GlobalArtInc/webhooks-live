use std::borrow::Cow;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::error::RedactError;
use crate::name::SecretName;
use crate::value::Secret;

pub const MIN_REDACTED_VALUE_BYTES: usize = 4;
pub const MAX_REDACT_SECRETS: usize = 32;
pub const MAX_REDACT_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_JSON_DEPTH: usize = 32;
pub const OVERSIZE_PLACEHOLDER: &str = "[secret:*]";

const JSON_NODE_COST: usize = 8;
const UNMARKED: u16 = u16::MAX;
const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

struct Needle {
    text: Zeroizing<String>,
    owner: u16,
}

pub struct Redactor {
    labels: Vec<String>,
    needles: Vec<Needle>,
}

#[derive(Clone, Copy)]
struct PercentStyle {
    keep_sub_delims: bool,
    upper: bool,
    space_as_plus: bool,
}

const PERCENT_STYLES: [PercentStyle; 5] = [
    PercentStyle {
        keep_sub_delims: false,
        upper: true,
        space_as_plus: false,
    },
    PercentStyle {
        keep_sub_delims: false,
        upper: false,
        space_as_plus: false,
    },
    PercentStyle {
        keep_sub_delims: true,
        upper: true,
        space_as_plus: false,
    },
    PercentStyle {
        keep_sub_delims: true,
        upper: false,
        space_as_plus: false,
    },
    PercentStyle {
        keep_sub_delims: false,
        upper: true,
        space_as_plus: true,
    },
];

impl Redactor {
    pub fn new<'a>(
        secrets: impl IntoIterator<Item = (&'a SecretName, &'a Secret)>,
    ) -> Result<Self, RedactError> {
        let mut labels = Vec::new();
        let mut needles: Vec<Needle> = Vec::new();
        for (index, (name, secret)) in secrets.into_iter().enumerate() {
            if index >= MAX_REDACT_SECRETS {
                return Err(RedactError::TooManySecrets);
            }
            let owner = u16::try_from(index).map_err(|_| RedactError::TooManySecrets)?;
            labels.push(format!("[secret:{name}]"));
            if secret.unpack().len() < MIN_REDACTED_VALUE_BYTES {
                continue;
            }
            for form in forms(secret.unpack()) {
                if !needles.iter().any(|needle| *needle.text == *form) {
                    needles.push(Needle { text: form, owner });
                }
            }
        }
        needles.sort_by_key(|needle| std::cmp::Reverse(needle.text.len()));
        Ok(Self { labels, needles })
    }

    pub fn is_empty(&self) -> bool {
        self.needles.is_empty()
    }

    pub fn redact_str<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.needles.is_empty() {
            return Cow::Borrowed(text);
        }
        if text.len() > MAX_REDACT_INPUT_BYTES {
            return Cow::Borrowed(OVERSIZE_PLACEHOLDER);
        }
        let mut marks: Option<Vec<u16>> = None;
        for needle in &self.needles {
            let width = needle.text.len();
            if width > text.len() {
                continue;
            }
            for (start, _) in text.match_indices(needle.text.as_str()) {
                let marks = marks.get_or_insert_with(|| vec![UNMARKED; text.len()]);
                if let Some(span) = marks.get_mut(start..start + width) {
                    for slot in span.iter_mut().filter(|slot| **slot == UNMARKED) {
                        *slot = needle.owner;
                    }
                }
            }
        }
        match marks {
            Some(marks) => Cow::Owned(self.assemble(text, &marks)),
            None => Cow::Borrowed(text),
        }
    }

    pub fn redact_json(&self, value: &Value) -> Value {
        if self.needles.is_empty() {
            return value.clone();
        }
        let mut budget = MAX_REDACT_INPUT_BYTES;
        self.walk(value, 0, &mut budget)
    }

    fn assemble(&self, text: &str, marks: &[u16]) -> String {
        let mut output = String::with_capacity(text.len());
        let mut copied = 0;
        let mut index = 0;
        while index < marks.len() {
            let owner = marks[index];
            if owner == UNMARKED {
                index += 1;
                continue;
            }
            output.push_str(text.get(copied..index).unwrap_or_default());
            let mut end = index;
            while marks.get(end) == Some(&owner) {
                end += 1;
            }
            output.push_str(self.label(owner));
            copied = end;
            index = end;
        }
        output.push_str(text.get(copied..).unwrap_or_default());
        output
    }

    fn label(&self, owner: u16) -> &str {
        self.labels
            .get(usize::from(owner))
            .map_or(OVERSIZE_PLACEHOLDER, String::as_str)
    }

    fn walk(&self, value: &Value, depth: usize, budget: &mut usize) -> Value {
        if depth > MAX_JSON_DEPTH || !spend(budget, JSON_NODE_COST) {
            return oversize();
        }
        match value {
            Value::Null | Value::Bool(_) => value.clone(),
            Value::Number(number) => {
                let text = number.to_string();
                if !spend(budget, text.len()) {
                    return oversize();
                }
                match self.redact_str(&text) {
                    Cow::Borrowed(_) => value.clone(),
                    Cow::Owned(redacted) => Value::String(redacted),
                }
            }
            Value::String(text) => match self.redact_within(text, budget) {
                Some(redacted) => Value::String(redacted),
                None => oversize(),
            },
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.walk(item, depth + 1, budget))
                    .collect(),
            ),
            Value::Object(entries) => {
                let mut redacted = Map::new();
                for (key, item) in entries {
                    let Some(key) = self.redact_within(key, budget) else {
                        redacted.insert(OVERSIZE_PLACEHOLDER.to_owned(), oversize());
                        break;
                    };
                    redacted.insert(key, self.walk(item, depth + 1, budget));
                }
                Value::Object(redacted)
            }
        }
    }

    fn redact_within(&self, text: &str, budget: &mut usize) -> Option<String> {
        spend(budget, text.len()).then(|| self.redact_str(text).into_owned())
    }
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redactor")
            .field("secrets", &self.labels.len())
            .field("forms", &self.needles.len())
            .finish_non_exhaustive()
    }
}

fn oversize() -> Value {
    Value::String(OVERSIZE_PLACEHOLDER.to_owned())
}

fn spend(budget: &mut usize, cost: usize) -> bool {
    if *budget >= cost {
        *budget -= cost;
        true
    } else {
        *budget = 0;
        false
    }
}

fn forms(value: &str) -> Vec<Zeroizing<String>> {
    let bytes = value.as_bytes();
    let mut forms = vec![Zeroizing::new(value.to_owned())];
    forms.push(Zeroizing::new(STANDARD.encode(bytes)));
    forms.push(Zeroizing::new(STANDARD_NO_PAD.encode(bytes)));
    forms.push(Zeroizing::new(URL_SAFE.encode(bytes)));
    forms.push(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes)));
    for style in PERCENT_STYLES {
        forms.push(Zeroizing::new(percent_encode(bytes, style)));
    }
    let mut distinct: Vec<Zeroizing<String>> = Vec::with_capacity(forms.len());
    for form in forms {
        if !distinct.iter().any(|seen| **seen == *form) {
            distinct.push(form);
        }
    }
    distinct
}

fn percent_encode(bytes: &[u8], style: PercentStyle) -> String {
    let digits = if style.upper { HEX_UPPER } else { HEX_LOWER };
    let mut encoded = String::with_capacity(bytes.len() * 3);
    for &byte in bytes {
        let kept = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~')
            || (style.keep_sub_delims && matches!(byte, b'!' | b'*' | b'\'' | b'(' | b')'));
        if kept {
            encoded.push(char::from(byte));
        } else if byte == b' ' && style.space_as_plus {
            encoded.push('+');
        } else {
            encoded.push('%');
            encoded.push(char::from(digits[usize::from(byte >> 4)]));
            encoded.push(char::from(digits[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}
