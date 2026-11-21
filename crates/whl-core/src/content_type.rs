use crate::header::is_token_byte;
use crate::validation::{InvalidValue, ViolationCode};

pub const MAX_CONTENT_TYPE_BYTES: usize = 255;
pub const DEFAULT_CONTENT_TYPE: &str = "text/plain";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentType(String);

impl ContentType {
    pub fn parse(text: &str) -> Result<Self, InvalidValue> {
        let mut codes = Vec::new();
        if text.is_empty() {
            codes.push(ViolationCode::Empty);
        } else {
            if text.len() > MAX_CONTENT_TYPE_BYTES {
                codes.push(ViolationCode::TooLong {
                    max: MAX_CONTENT_TYPE_BYTES,
                });
            }
            if text.chars().any(char::is_control) {
                codes.push(ViolationCode::ControlCharacter);
            } else if !is_media_type(text.as_bytes()) {
                codes.push(ViolationCode::InvalidMediaType);
            }
        }
        match InvalidValue::from_codes(codes) {
            Some(error) => Err(error),
            None => Ok(Self(text.to_owned())),
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl Default for ContentType {
    fn default() -> Self {
        Self(DEFAULT_CONTENT_TYPE.to_owned())
    }
}

fn is_media_type(bytes: &[u8]) -> bool {
    let essence_end = bytes
        .iter()
        .position(|&byte| byte == b';')
        .unwrap_or(bytes.len());
    let (essence, parameters) = bytes.split_at(essence_end);
    let essence = if parameters.is_empty() {
        essence
    } else {
        trim_trailing_spaces(essence)
    };
    is_essence(essence) && are_parameters(parameters)
}

fn is_essence(essence: &[u8]) -> bool {
    let Some(slash) = essence.iter().position(|&byte| byte == b'/') else {
        return false;
    };
    let (kind, subtype) = essence.split_at(slash);
    is_media_token(kind) && is_media_token(&subtype[1..])
}

fn is_media_token(token: &[u8]) -> bool {
    !token.is_empty()
        && token
            .iter()
            .all(|&byte| is_token_byte(byte) && byte != b'*')
}

fn are_parameters(mut rest: &[u8]) -> bool {
    while !rest.is_empty() {
        let Some(after_semicolon) = rest.strip_prefix(b";") else {
            return false;
        };
        let after_space = skip_spaces(after_semicolon);
        if after_space.is_empty() {
            return true;
        }
        let Some(equals) = after_space.iter().position(|&byte| byte == b'=') else {
            return false;
        };
        let (name, after_name) = after_space.split_at(equals);
        if name.is_empty() || !name.iter().all(|&byte| is_token_byte(byte)) {
            return false;
        }
        let value = &after_name[1..];
        let Some(remaining) = take_parameter_value(value) else {
            return false;
        };
        rest = skip_spaces(remaining);
        if rest.is_empty() && !remaining.is_empty() {
            return false;
        }
    }
    true
}

fn trim_trailing_spaces(bytes: &[u8]) -> &[u8] {
    let spaces = bytes.iter().rev().take_while(|&&byte| byte == b' ').count();
    &bytes[..bytes.len() - spaces]
}

fn skip_spaces(bytes: &[u8]) -> &[u8] {
    let spaces = bytes.iter().take_while(|&&byte| byte == b' ').count();
    &bytes[spaces..]
}

fn take_parameter_value(value: &[u8]) -> Option<&[u8]> {
    if value.first() == Some(&b'"') {
        take_quoted_string(&value[1..])
    } else {
        let length = value
            .iter()
            .take_while(|&&byte| is_token_byte(byte))
            .count();
        (length > 0).then(|| &value[length..])
    }
}

fn take_quoted_string(mut rest: &[u8]) -> Option<&[u8]> {
    loop {
        let (&byte, tail) = rest.split_first()?;
        match byte {
            b'"' => return Some(tail),
            b'\\' => {
                let (&escaped, after) = tail.split_first()?;
                if !is_visible_or_space(escaped) {
                    return None;
                }
                rest = after;
            }
            _ if is_visible_or_space(byte) => rest = tail,
            _ => return None,
        }
    }
}

fn is_visible_or_space(byte: u8) -> bool {
    (0x20..=0x7e).contains(&byte)
}
