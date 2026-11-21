use std::borrow::Cow;

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

const LENIENT_PADDING: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FilterFailure {
    TooLarge,
    InvalidBase64,
    InvalidKey,
}

pub(crate) fn upper(text: &str, max: usize) -> Result<String, FilterFailure> {
    bounded(text.to_uppercase(), max)
}

pub(crate) fn lower(text: &str, max: usize) -> Result<String, FilterFailure> {
    bounded(text.to_lowercase(), max)
}

fn bounded(text: String, max: usize) -> Result<String, FilterFailure> {
    if text.len() > max {
        Err(FilterFailure::TooLarge)
    } else {
        Ok(text)
    }
}

pub(crate) fn trim(text: Cow<'_, str>) -> Cow<'_, str> {
    match text {
        Cow::Borrowed(borrowed) => Cow::Borrowed(borrowed.trim()),
        Cow::Owned(owned) => {
            let trimmed = owned.trim();
            if trimmed.len() == owned.len() {
                Cow::Owned(owned)
            } else {
                Cow::Owned(trimmed.to_owned())
            }
        }
    }
}

pub(crate) fn truncate(text: Cow<'_, str>, characters: usize) -> Cow<'_, str> {
    let Some((index, _)) = text.char_indices().nth(characters) else {
        return text;
    };
    match text {
        Cow::Borrowed(borrowed) => Cow::Borrowed(borrowed.get(..index).unwrap_or(borrowed)),
        Cow::Owned(mut owned) => {
            owned.truncate(index);
            Cow::Owned(owned)
        }
    }
}

pub(crate) fn replace(
    text: &str,
    from: &str,
    to: &str,
    max: usize,
) -> Result<String, FilterFailure> {
    let matches = text.matches(from).count();
    let removed = matches.saturating_mul(from.len());
    let added = matches.saturating_mul(to.len());
    let size = text.len().saturating_sub(removed).saturating_add(added);
    if size > max {
        return Err(FilterFailure::TooLarge);
    }
    Ok(text.replace(from, to))
}

pub(crate) fn length(text: &str) -> i64 {
    i64::try_from(text.chars().count()).unwrap_or(i64::MAX)
}

pub(crate) fn push_json_escaped(
    text: &str,
    out: &mut String,
    max_total: usize,
) -> Result<(), FilterFailure> {
    for character in text.chars() {
        let mut buffer = [0u8; 4];
        let escaped: Cow<'_, str> = match character {
            '"' => Cow::Borrowed("\\\""),
            '\\' => Cow::Borrowed("\\\\"),
            '\n' => Cow::Borrowed("\\n"),
            '\r' => Cow::Borrowed("\\r"),
            '\t' => Cow::Borrowed("\\t"),
            '\u{8}' => Cow::Borrowed("\\b"),
            '\u{c}' => Cow::Borrowed("\\f"),
            '\u{2028}' => Cow::Borrowed("\\u2028"),
            '\u{2029}' => Cow::Borrowed("\\u2029"),
            control if (control as u32) < 0x20 => Cow::Owned(format!("\\u{:04x}", control as u32)),
            plain => Cow::Borrowed(&*plain.encode_utf8(&mut buffer)),
        };
        if out.len().saturating_add(escaped.len()) > max_total {
            return Err(FilterFailure::TooLarge);
        }
        out.push_str(&escaped);
    }
    Ok(())
}

pub(crate) fn json_string(text: &str, max: usize) -> Result<String, FilterFailure> {
    let mut out = String::with_capacity(text.len().min(max).saturating_add(2));
    out.push('"');
    push_json_escaped(text, &mut out, max.saturating_sub(1))?;
    out.push('"');
    bounded(out, max)
}

pub(crate) fn url_encode(text: &str, max: usize) -> Result<String, FilterFailure> {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len().min(max));
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(DIGITS[usize::from(byte >> 4)]));
            out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        if out.len() > max {
            return Err(FilterFailure::TooLarge);
        }
    }
    Ok(out)
}

pub(crate) fn html_escape(text: &str, max: usize) -> Result<String, FilterFailure> {
    let mut out = String::with_capacity(text.len().min(max));
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            plain => out.push(plain),
        }
        if out.len() > max {
            return Err(FilterFailure::TooLarge);
        }
    }
    Ok(out)
}

pub(crate) fn base64_encode(text: &str, max: usize) -> Result<String, FilterFailure> {
    let size = text.len().div_ceil(3).saturating_mul(4);
    if size > max {
        return Err(FilterFailure::TooLarge);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(text.as_bytes()))
}

pub(crate) fn base64_decode(text: &str, max: usize) -> Result<String, FilterFailure> {
    let trimmed = text.trim();
    if trimmed.len().div_ceil(4).saturating_mul(3) > max.saturating_add(2) {
        return Err(FilterFailure::TooLarge);
    }
    let bytes = LENIENT_PADDING
        .decode(trimmed.as_bytes())
        .map_err(|_| FilterFailure::InvalidBase64)?;
    if bytes.len() > max {
        return Err(FilterFailure::TooLarge);
    }
    bounded(String::from_utf8_lossy(&bytes).into_owned(), max)
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

pub(crate) fn hex_text(text: &str, max: usize) -> Result<String, FilterFailure> {
    if text.len().saturating_mul(2) > max {
        return Err(FilterFailure::TooLarge);
    }
    Ok(hex_encode(text.as_bytes()))
}

pub(crate) fn sha256_hex(text: &str) -> String {
    hex_encode(&Sha256::digest(text.as_bytes()))
}

pub(crate) fn hmac_sha256_hex(key: &str, text: &str) -> Result<String, FilterFailure> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_bytes())
        .map_err(|_| FilterFailure::InvalidKey)?;
    mac.update(text.as_bytes());
    Ok(hex_encode(&mac.finalize().into_bytes()))
}
