use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use regex::{Regex, RegexBuilder};
use rhai::{Array, Dynamic, Engine, EvalAltResult, ImmutableString, Position};
use sha2::{Digest, Sha256};

use crate::convert::{
    BoundedWriter, ConvertError, ConvertLimits, dynamic_to_json, json_to_dynamic,
};
use crate::env::RandomSource;
use crate::guard::RunGuard;

const MAX_REGEX_PATTERN_BYTES: usize = 512;
const MAX_REGEX_PATTERNS: usize = 8;
const REGEX_COMPILE_COST: usize = 16 * 1024;
const REGEX_NEST_LIMIT: u32 = 32;
const REGEX_PROBE_START_BYTES: usize = 8 * 1024;
const REGEX_DFA_BYTES: usize = 256 * 1024;
const REGEX_CALL_UNITS: u64 = 256 * 1024;
const REGEX_UNIT_COST: usize = 4;
const JSON_PARSE_MAX_DEPTH: usize = 32;
const JSON_PARSE_MAX_NODES: usize = 4096;
const MAX_RANDOM_REJECTIONS: usize = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) struct HostLimits {
    pub max_string_bytes: usize,
    pub max_array_size: usize,
    pub max_map_size: usize,
    pub max_text_bytes: usize,
    pub regex_size_limit_bytes: usize,
}

struct CompiledRegex {
    regex: Regex,
    program_kib: u64,
}

pub(crate) struct HostState {
    guard: Arc<RunGuard>,
    random: Mutex<Box<dyn RandomSource>>,
    now: String,
    regexes: Mutex<HashMap<String, Arc<CompiledRegex>>>,
    limits: HostLimits,
}

struct NoRandom;

impl RandomSource for NoRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        destination.fill(0);
    }
}

type HostResult<T> = Result<T, Box<EvalAltResult>>;

fn runtime(message: &str) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.to_owned()),
        Position::NONE,
    ))
}

fn too_large(what: &str) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorDataTooLarge(
        what.to_owned(),
        Position::NONE,
    ))
}

fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

impl HostState {
    pub(crate) fn new(guard: Arc<RunGuard>, now: String, limits: HostLimits) -> Self {
        Self {
            guard,
            random: Mutex::new(Box::new(NoRandom)),
            now,
            regexes: Mutex::new(HashMap::new()),
            limits,
        }
    }

    fn input(&self, text: &str, cost_factor: usize) -> HostResult<()> {
        if text.len() > self.limits.max_text_bytes {
            return Err(too_large("host function input"));
        }
        self.guard.charge(text.len().saturating_mul(cost_factor))
    }

    fn output(&self, text: String) -> HostResult<String> {
        if text.len() > self.limits.max_string_bytes {
            return Err(too_large("string"));
        }
        Ok(text)
    }

    fn convert_limits(&self) -> ConvertLimits {
        ConvertLimits {
            max_depth: JSON_PARSE_MAX_DEPTH,
            max_nodes: JSON_PARSE_MAX_NODES,
            max_bytes: self.limits.max_text_bytes,
            max_string_bytes: self.limits.max_string_bytes,
            max_array_len: self.limits.max_array_size,
            max_map_len: self.limits.max_map_size,
        }
    }

    pub(crate) fn swap_random(&self, slot: &mut Box<dyn RandomSource>) {
        let mut random = self.random.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::swap(&mut *random, slot);
    }

    fn fill(&self, destination: &mut [u8]) {
        let mut random = self.random.lock().unwrap_or_else(PoisonError::into_inner);
        random.fill_bytes(destination);
    }

    fn json_parse(&self, text: &str) -> HostResult<Dynamic> {
        self.input(text, 4)?;
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| runtime(&format!("invalid_json: {}", json_label(&error))))?;
        json_to_dynamic(&value, self.convert_limits()).map_err(convert_failure)
    }

    fn json_stringify(&self, value: &Dynamic) -> HostResult<String> {
        self.guard.charge(1024)?;
        let json = dynamic_to_json(value, self.convert_limits()).map_err(convert_failure)?;
        let mut writer = BoundedWriter::new(self.limits.max_string_bytes);
        serde_json::to_writer(&mut writer, &json).map_err(|_| too_large("string"))?;
        writer
            .into_string()
            .ok_or_else(|| runtime("invalid_json: output is not text"))
    }

    fn base64_decode(&self, text: &str) -> HostResult<String> {
        self.input(text, 2)?;
        let bytes = STANDARD
            .decode(text.trim().as_bytes())
            .map_err(|_| runtime("invalid_base64: input is not valid base64"))?;
        let decoded = String::from_utf8(bytes)
            .map_err(|_| runtime("invalid_base64: decoded bytes are not UTF-8 text"))?;
        self.output(decoded)
    }

    fn regex(&self, pattern: &str) -> HostResult<Arc<CompiledRegex>> {
        if pattern.len() > MAX_REGEX_PATTERN_BYTES {
            return Err(runtime("invalid_regex: pattern is too long"));
        }
        let mut cache = self.regexes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = cache.get(pattern) {
            return Ok(Arc::clone(found));
        }
        self.guard.charge(REGEX_COMPILE_COST)?;
        let ceiling = self.limits.regex_size_limit_bytes;
        let mut size = REGEX_PROBE_START_BYTES.min(ceiling);
        let compiled = loop {
            let built = RegexBuilder::new(pattern)
                .size_limit(size)
                .dfa_size_limit(REGEX_DFA_BYTES)
                .nest_limit(REGEX_NEST_LIMIT)
                .build();
            match built {
                Ok(regex) => {
                    break CompiledRegex {
                        regex,
                        program_kib: u64::try_from(size / 1024).unwrap_or(u64::MAX).max(1),
                    };
                }
                Err(regex::Error::CompiledTooBig(_)) if size < ceiling => {
                    size = size.saturating_mul(2).min(ceiling);
                }
                Err(regex::Error::CompiledTooBig(_)) => {
                    return Err(runtime("invalid_regex: pattern is too large"));
                }
                Err(_) => return Err(runtime("invalid_regex: pattern is invalid")),
            }
        };
        if cache.len() >= MAX_REGEX_PATTERNS {
            cache.clear();
        }
        let compiled = Arc::new(compiled);
        cache.insert(pattern.to_owned(), Arc::clone(&compiled));
        Ok(compiled)
    }

    fn regex_work(&self, regex: &CompiledRegex, text: &str) -> HostResult<()> {
        let units = u64::try_from(text.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(regex.program_kib);
        if units > REGEX_CALL_UNITS {
            return Err(runtime(
                "regex_too_costly: the pattern is too complex for a text this long",
            ));
        }
        let bytes = usize::try_from(units).unwrap_or(usize::MAX);
        self.guard.charge(bytes.saturating_mul(REGEX_UNIT_COST))
    }

    fn regex_match(&self, pattern: &str, text: &str) -> HostResult<bool> {
        self.input(text, 0)?;
        let compiled = self.regex(pattern)?;
        self.regex_work(&compiled, text)?;
        let found = compiled.regex.is_match(text);
        self.guard.charge(0)?;
        Ok(found)
    }

    fn regex_captures(&self, pattern: &str, text: &str) -> HostResult<Array> {
        self.input(text, 0)?;
        let compiled = self.regex(pattern)?;
        self.regex_work(&compiled, text)?;
        let captures = compiled.regex.captures(text);
        self.guard.charge(0)?;
        let Some(captures) = captures else {
            return Ok(Array::new());
        };
        if captures.len() > self.limits.max_array_size {
            return Err(too_large("array"));
        }
        let mut groups = Array::with_capacity(captures.len());
        for group in captures.iter() {
            groups.push(match group {
                Some(found) => {
                    if found.as_str().len() > self.limits.max_string_bytes {
                        return Err(too_large("string"));
                    }
                    Dynamic::from(found.as_str().to_owned())
                }
                None => Dynamic::UNIT,
            });
        }
        Ok(groups)
    }

    fn replace(&self, text: &mut ImmutableString, find: &str, substitute: &str) -> HostResult<()> {
        self.guard.charge(text.len())?;
        if text.is_empty() {
            return Ok(());
        }
        let occurrences = text.matches(find).count();
        let removed = occurrences.saturating_mul(find.len());
        let added = occurrences.saturating_mul(substitute.len());
        let grown = text.len().saturating_sub(removed).saturating_add(added);
        if grown > self.limits.max_string_bytes {
            return Err(too_large("string"));
        }
        *text = text.replace(find, substitute).into();
        Ok(())
    }

    fn hmac_sha256(&self, key: &str, text: &str) -> HostResult<String> {
        self.input(text, 2)?;
        self.input(key, 2)?;
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_bytes())
            .map_err(|_| runtime("invalid_key: the HMAC key was rejected"))?;
        mac.update(text.as_bytes());
        Ok(hex_encode(&mac.finalize().into_bytes()))
    }

    fn uuid(&self) -> HostResult<String> {
        self.guard.charge(64)?;
        let mut bytes = [0u8; 16];
        self.fill(&mut bytes);
        Ok(uuid::Builder::from_random_bytes(bytes)
            .into_uuid()
            .hyphenated()
            .to_string())
    }

    fn random_int(&self, low: i64, high: i64) -> HostResult<i64> {
        self.guard.charge(64)?;
        if low > high {
            return Err(runtime(
                "invalid_argument: random_int lower bound is above the upper bound",
            ));
        }
        let span = i128::from(high) - i128::from(low) + 1;
        let span = u128::try_from(span).unwrap_or(1);
        if span > u128::from(u64::MAX) {
            let mut bytes = [0u8; 8];
            self.fill(&mut bytes);
            return Ok(i64::from_le_bytes(bytes));
        }
        let span = u64::try_from(span).unwrap_or(1);
        let zone = u64::MAX - (u64::MAX % span);
        let mut draw = 0u64;
        for _ in 0..MAX_RANDOM_REJECTIONS {
            let mut bytes = [0u8; 8];
            self.fill(&mut bytes);
            draw = u64::from_le_bytes(bytes);
            if draw < zone {
                break;
            }
        }
        let offset = i128::from(draw % span);
        i64::try_from(i128::from(low) + offset)
            .map_err(|_| runtime("invalid_argument: random_int result is out of range"))
    }
}

fn json_label(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Io => "input error",
        serde_json::error::Category::Syntax => "syntax error",
        serde_json::error::Category::Data => "data error",
        serde_json::error::Category::Eof => "unexpected end of input",
    }
}

fn convert_failure(error: ConvertError) -> Box<EvalAltResult> {
    match error {
        ConvertError::TooDeep(_)
        | ConvertError::TooManyNodes(_)
        | ConvertError::TooManyBytes(_)
        | ConvertError::StringTooLarge(_)
        | ConvertError::ArrayTooLarge(_)
        | ConvertError::MapTooLarge(_) => too_large("json value"),
        ConvertError::NotFinite | ConvertError::Unsupported(_) => {
            runtime(&format!("invalid_json: {error}"))
        }
    }
}

pub(crate) fn register(engine: &mut Engine, state: &Arc<HostState>) {
    let host = Arc::clone(state);
    engine.register_fn("json_parse", move |text: &str| host.json_parse(text));
    let host = Arc::clone(state);
    engine.register_fn("json_stringify", move |value: Dynamic| {
        host.json_stringify(&value)
    });
    let host = Arc::clone(state);
    engine.register_fn("base64_encode", move |text: &str| -> HostResult<String> {
        host.input(text, 2)?;
        host.output(STANDARD.encode(text.as_bytes()))
    });
    let host = Arc::clone(state);
    engine.register_fn("base64_decode", move |text: &str| host.base64_decode(text));
    let host = Arc::clone(state);
    engine.register_fn("hex", move |text: &str| -> HostResult<String> {
        host.input(text, 2)?;
        host.output(hex_encode(text.as_bytes()))
    });
    let host = Arc::clone(state);
    engine.register_fn("sha256", move |text: &str| -> HostResult<String> {
        host.input(text, 2)?;
        Ok(hex_encode(&Sha256::digest(text.as_bytes())))
    });
    let host = Arc::clone(state);
    engine.register_fn("hmac_sha256", move |key: &str, text: &str| {
        host.hmac_sha256(key, text)
    });
    let host = Arc::clone(state);
    engine.register_fn("regex_match", move |pattern: &str, text: &str| {
        host.regex_match(pattern, text)
    });
    let host = Arc::clone(state);
    engine.register_fn("regex_captures", move |pattern: &str, text: &str| {
        host.regex_captures(pattern, text)
    });
    let host = Arc::clone(state);
    engine.register_fn(
        "replace",
        move |text: &mut ImmutableString, find: &str, substitute: &str| {
            host.replace(text, find, substitute)
        },
    );
    let host = Arc::clone(state);
    engine.register_fn(
        "replace",
        move |text: &mut ImmutableString, find: &str, substitute: char| {
            host.replace(text, find, &substitute.to_string())
        },
    );
    let host = Arc::clone(state);
    engine.register_fn(
        "replace",
        move |text: &mut ImmutableString, find: char, substitute: &str| {
            host.replace(text, &find.to_string(), substitute)
        },
    );
    let host = Arc::clone(state);
    engine.register_fn(
        "replace",
        move |text: &mut ImmutableString, find: char, substitute: char| {
            host.replace(text, &find.to_string(), &substitute.to_string())
        },
    );
    let host = Arc::clone(state);
    engine.register_fn("uuid", move || host.uuid());
    let host = Arc::clone(state);
    engine.register_fn("random_int", move |low: i64, high: i64| {
        host.random_int(low, high)
    });
    let host = Arc::clone(state);
    engine.register_fn("now", move || -> HostResult<String> {
        host.guard.charge(32)?;
        Ok(host.now.clone())
    });
}
