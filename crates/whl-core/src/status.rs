use std::fmt;

pub const STATUS_MIN: u16 = 100;
pub const STATUS_MAX: u16 = 599;

const OVERRIDE_DIGITS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HttpStatus(u16);

impl HttpStatus {
    pub fn new(code: i64) -> Option<Self> {
        let code = u16::try_from(code).ok()?;
        (STATUS_MIN..=STATUS_MAX)
            .contains(&code)
            .then_some(Self(code))
    }

    pub fn unpack(&self) -> u16 {
        self.0
    }
}

impl Default for HttpStatus {
    fn default() -> Self {
        Self(200)
    }
}

impl fmt::Display for HttpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn parse_status_override(first_segment: &str) -> Option<HttpStatus> {
    let bytes = first_segment.as_bytes();
    if bytes.len() != OVERRIDE_DIGITS || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let code = bytes
        .iter()
        .fold(0_i64, |acc, digit| acc * 10 + i64::from(digit - b'0'));
    HttpStatus::new(code)
}

pub fn first_segment_after_id(path_after_id: &str) -> Option<&str> {
    let rest = path_after_id.strip_prefix('/')?;
    Some(rest.split('/').next().unwrap_or_default())
}

pub fn status_override_from_path(path_after_id: &str) -> Option<HttpStatus> {
    parse_status_override(first_segment_after_id(path_after_id)?)
}
