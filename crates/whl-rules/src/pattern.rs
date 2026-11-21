use std::time::Duration;

use regex::{Captures, Regex, RegexBuilder};
use thiserror::Error;

use crate::limits::{
    CAPTURE_COST_FACTOR, MAX_EXTRACT_CAPTURES, MAX_REGEX_DFA_BYTES, MAX_REGEX_HAYSTACK_BYTES,
    MAX_REGEX_PATTERN_BYTES, MAX_REGEX_REPETITION, MAX_REGEX_SIZE_BYTES,
};

const NEST_LIMIT: u32 = 32;
const PROBE_START_BYTES: usize = 1024;
const UNIT_NANOS: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PatternError {
    #[error("must be at most {MAX_REGEX_PATTERN_BYTES} bytes")]
    TooLong,
    #[error("must not repeat anything more than {MAX_REGEX_REPETITION} times")]
    Repetition,
    #[error("must be a valid regular expression")]
    Invalid,
    #[error("is too complex")]
    TooLarge,
    #[error(
        "must not capture groups in a condition (write (?:...) instead) and at most {} in an extract",
        MAX_EXTRACT_CAPTURES - 1
    )]
    Groups,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternUse {
    Condition,
    Extract,
}

#[derive(Debug)]
pub struct Pattern {
    regex: Regex,
    program_kib: u64,
}

pub fn bounded(text: &str) -> &str {
    if text.len() <= MAX_REGEX_HAYSTACK_BYTES {
        return text;
    }
    let mut end = MAX_REGEX_HAYSTACK_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

fn skip_braced(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index] != b'}' {
        index += 1;
    }
    index + 1
}

fn parse_number(bytes: &[u8], index: &mut usize) -> Option<u64> {
    let start = *index;
    let mut value: u64 = 0;
    while let Some(digit) = bytes.get(*index).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(u64::from(digit - b'0'));
        *index += 1;
    }
    (*index > start).then_some(value)
}

fn skip_space(bytes: &[u8], index: &mut usize) {
    while bytes
        .get(*index)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        *index += 1;
    }
}

fn counted_end(bytes: &[u8], open: usize, largest: &mut u64) -> Option<usize> {
    let mut index = open + 1;
    skip_space(bytes, &mut index);
    let low = parse_number(bytes, &mut index)?;
    *largest = (*largest).max(low);
    skip_space(bytes, &mut index);
    if bytes.get(index) == Some(&b',') {
        index += 1;
        skip_space(bytes, &mut index);
        if let Some(high) = parse_number(bytes, &mut index) {
            *largest = (*largest).max(high);
        }
        skip_space(bytes, &mut index);
    }
    (bytes.get(index) == Some(&b'}')).then_some(index + 1)
}

pub fn largest_repetition(pattern: &str) -> u64 {
    let bytes = pattern.as_bytes();
    let mut largest = 0;
    let mut classes = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                let escaped = bytes.get(index + 1).copied();
                if matches!(escaped, Some(b'x' | b'u' | b'U' | b'p' | b'P' | b'N'))
                    && bytes.get(index + 2) == Some(&b'{')
                {
                    index = skip_braced(bytes, index + 3);
                } else {
                    index += 2;
                }
            }
            b'[' => {
                classes += 1;
                index += 1;
                if bytes.get(index) == Some(&b'^') {
                    index += 1;
                }
                if bytes.get(index) == Some(&b']') {
                    index += 1;
                }
            }
            b']' if classes > 0 => {
                classes -= 1;
                index += 1;
            }
            b'{' if classes == 0 => {
                index = counted_end(bytes, index, &mut largest).unwrap_or(index + 1);
            }
            _ => index += 1,
        }
    }
    largest
}

pub fn check(pattern: &str) -> Result<(), PatternError> {
    if pattern.len() > MAX_REGEX_PATTERN_BYTES {
        return Err(PatternError::TooLong);
    }
    if largest_repetition(pattern) > u64::from(MAX_REGEX_REPETITION) {
        return Err(PatternError::Repetition);
    }
    Ok(())
}

impl Pattern {
    pub fn compile(pattern: &str, usage: PatternUse) -> Result<Self, PatternError> {
        check(pattern)?;
        let mut size = PROBE_START_BYTES.min(MAX_REGEX_SIZE_BYTES);
        loop {
            let built = RegexBuilder::new(pattern)
                .size_limit(size)
                .dfa_size_limit(MAX_REGEX_DFA_BYTES)
                .nest_limit(NEST_LIMIT)
                .build();
            match built {
                Ok(regex) => {
                    let groups = regex.captures_len();
                    let allowed = match usage {
                        PatternUse::Condition => groups == 1,
                        PatternUse::Extract => groups <= MAX_EXTRACT_CAPTURES,
                    };
                    if !allowed {
                        return Err(PatternError::Groups);
                    }
                    return Ok(Self {
                        regex,
                        program_kib: u64::try_from(size / 1024).unwrap_or(u64::MAX).max(1),
                    });
                }
                Err(regex::Error::CompiledTooBig(_)) if size < MAX_REGEX_SIZE_BYTES => {
                    size = size.saturating_mul(2).min(MAX_REGEX_SIZE_BYTES);
                }
                Err(regex::Error::CompiledTooBig(_)) => return Err(PatternError::TooLarge),
                Err(_) => return Err(PatternError::Invalid),
            }
        }
    }

    pub fn weight_bytes(&self) -> usize {
        usize::try_from(self.program_kib)
            .unwrap_or(usize::MAX)
            .saturating_mul(2 * 1024)
            .saturating_add(8 * 1024)
    }

    pub fn cost(&self, haystack_len: usize) -> Duration {
        let units = u64::try_from(haystack_len.min(MAX_REGEX_HAYSTACK_BYTES))
            .unwrap_or(u64::MAX)
            .saturating_mul(self.program_kib);
        Duration::from_nanos(units.saturating_mul(UNIT_NANOS))
    }

    pub fn capture_cost(&self, haystack_len: usize) -> Duration {
        self.cost(haystack_len).saturating_mul(CAPTURE_COST_FACTOR)
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(bounded(text))
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        self.regex.captures(bounded(text))
    }
}
