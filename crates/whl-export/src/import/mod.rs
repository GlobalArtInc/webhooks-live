mod capture;
mod error;
mod header;
mod scan;
#[cfg(test)]
mod tests;

use std::error::Error;

use futures_util::{Stream, StreamExt};
use serde_json::Value;
use time::OffsetDateTime;
use whl_store::import::ImportCapture;

pub use error::{ImportError, ImportViolation, Place, Problem};
pub use header::ImportHeader;
pub use scan::MAX_DEPTH;

use header::HeaderBuilder;
use scan::{Fault, Scan, ValueScan, is_space};

pub const MEMBER_VALUE_BYTES: usize = 256 * 1024;
pub const KEY_BYTES: usize = 256;
pub const ELEMENT_OVERHEAD_BYTES: u64 = 1024 * 1024;
pub const ELEMENT_ESCAPE_FACTOR: u64 = 6;
const BLANK_LINES_PER_YIELD: usize = 4096;
const COLLECTION_KEY: &str = "captures";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    Json,
    Ndjson,
}

impl ImportFormat {
    pub fn from_content_type(header: &str) -> Option<Self> {
        let essence = header.split(';').next().unwrap_or_default().trim();
        if essence.eq_ignore_ascii_case("application/json") {
            Some(Self::Json)
        } else if essence.eq_ignore_ascii_case("application/x-ndjson") {
            Some(Self::Ndjson)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImportLimits {
    pub max_input_bytes: u64,
    pub max_body_bytes: u64,
    pub earliest: OffsetDateTime,
    pub latest: OffsetDateTime,
    pub enforce_time_range: bool,
}

impl ImportLimits {
    fn element_bytes(&self) -> usize {
        let cap = self
            .max_body_bytes
            .saturating_mul(ELEMENT_ESCAPE_FACTOR)
            .saturating_add(ELEMENT_OVERHEAD_BYTES);
        usize::try_from(cap).unwrap_or(usize::MAX)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Members { started: bool },
    Captures { first: bool },
    AfterCaptures,
    Lines,
    Done,
}

fn too_long(place: Place, cap: usize) -> ImportError {
    ImportError::invalid(
        place,
        "",
        Problem::TooLong,
        &format!("is longer than {cap} bytes"),
    )
}

struct Source<S> {
    stream: S,
    buffer: Vec<u8>,
    start: usize,
    base: u64,
    total: u64,
    max: u64,
    finished: bool,
}

impl<S, B, E> Source<S>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Error + Send + Sync + 'static,
{
    fn new(stream: S, max: u64) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            start: 0,
            base: 0,
            total: 0,
            max,
            finished: false,
        }
    }

    fn pending(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    fn offset(&self) -> u64 {
        self.base + self.start as u64
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
    }

    fn compact(&mut self) {
        if self.start == 0 {
            return;
        }
        if self.start == self.buffer.len() {
            self.buffer.clear();
        } else if self.start * 2 >= self.buffer.len() {
            self.buffer.drain(..self.start);
        } else {
            return;
        }
        self.base += self.start as u64;
        self.start = 0;
    }

    async fn fill(&mut self) -> Result<bool, ImportError> {
        if self.finished {
            return Ok(false);
        }
        self.compact();
        match self.stream.next().await {
            Some(Ok(chunk)) => {
                let chunk = chunk.as_ref();
                self.total = self.total.saturating_add(chunk.len() as u64);
                if self.total > self.max {
                    return Err(ImportError::TooLarge { max: self.max });
                }
                self.buffer.extend_from_slice(chunk);
                Ok(true)
            }
            Some(Err(error)) => Err(ImportError::Read(Box::new(error))),
            None => {
                self.finished = true;
                Ok(false)
            }
        }
    }
}

pub struct ImportReader<S> {
    source: Source<S>,
    format: ImportFormat,
    limits: ImportLimits,
    phase: Phase,
    index: usize,
    line: usize,
    scanned: usize,
}

impl<S, B, E> ImportReader<S>
where
    S: Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: Error + Send + Sync + 'static,
{
    pub async fn open(
        stream: S,
        format: ImportFormat,
        limits: ImportLimits,
    ) -> Result<(Self, ImportHeader), ImportError> {
        let mut reader = Self {
            source: Source::new(stream, limits.max_input_bytes),
            format,
            limits,
            phase: match format {
                ImportFormat::Json => Phase::Members { started: false },
                ImportFormat::Ndjson => Phase::Lines,
            },
            index: 0,
            line: 0,
            scanned: 0,
        };
        let header = match format {
            ImportFormat::Json => reader.json_header().await?,
            ImportFormat::Ndjson => reader.ndjson_header().await?,
        };
        Ok((reader, header))
    }

    pub async fn next_capture(&mut self) -> Result<Option<ImportCapture>, ImportError> {
        match self.format {
            ImportFormat::Json => self.json_capture().await,
            ImportFormat::Ndjson => self.ndjson_capture().await,
        }
    }

    fn document_error(&self, path: &str, problem: Problem, text: &str) -> ImportError {
        ImportError::invalid(Place::Document, path, problem, text)
    }

    fn syntax_error(&self, place: Place, offset: u64) -> ImportError {
        ImportError::invalid(
            place,
            "",
            Problem::InvalidFormat,
            &format!("is not valid JSON (near byte {offset})"),
        )
    }

    async fn peek(&mut self) -> Result<Option<u8>, ImportError> {
        loop {
            let pending = self.source.pending();
            if let Some(offset) = pending.iter().position(|byte| !is_space(*byte)) {
                self.source.consume(offset);
                return Ok(Some(self.source.pending()[0]));
            }
            let all = pending.len();
            self.source.consume(all);
            if !self.source.fill().await? {
                return Ok(None);
            }
        }
    }

    async fn expect(&mut self, wanted: u8, what: &str) -> Result<(), ImportError> {
        match self.peek().await? {
            Some(byte) if byte == wanted => {
                self.source.consume(1);
                Ok(())
            }
            Some(_) => Err(self.document_error(
                "",
                Problem::InvalidFormat,
                &format!(
                    "is not valid JSON: expected {what} near byte {}",
                    self.source.offset()
                ),
            )),
            None => Err(self.ended()),
        }
    }

    fn ended(&self) -> ImportError {
        self.document_error(
            "",
            Problem::InvalidFormat,
            "ends before the document is complete",
        )
    }

    async fn read_json(&mut self, cap: usize, place: Place) -> Result<Value, ImportError> {
        let offset = self.source.offset();
        let mut scan = ValueScan::new();
        loop {
            match scan.advance(self.source.pending(), self.source.finished) {
                Ok(Scan::Done(length)) if length > cap => return Err(too_long(place, cap)),
                Ok(Scan::Done(length)) => {
                    let parsed = serde_json::from_slice::<Value>(&self.source.pending()[..length]);
                    self.source.consume(length);
                    return parsed.map_err(|_| self.syntax_error(place, offset));
                }
                Ok(Scan::More) => {}
                Err(Fault::TooDeep) => {
                    return Err(ImportError::invalid(
                        place,
                        "",
                        Problem::OutOfRange,
                        "is nested too deeply",
                    ));
                }
                Err(Fault::Incomplete) => return Err(self.ended()),
            }
            if self.source.pending().len() > cap {
                return Err(too_long(place, cap));
            }
            self.source.fill().await?;
        }
    }

    async fn member_key(&mut self) -> Result<Option<String>, ImportError> {
        let Phase::Members { started } = self.phase else {
            return Ok(None);
        };
        let Some(next) = self.peek().await? else {
            return Err(self.ended());
        };
        match next {
            b'}' => {
                self.source.consume(1);
                self.phase = Phase::Done;
                return Ok(None);
            }
            b',' if started => {
                self.source.consume(1);
                match self.peek().await? {
                    Some(b'"') => {}
                    Some(_) => {
                        return Err(self.document_error(
                            "",
                            Problem::InvalidFormat,
                            "is not valid JSON: expected a member name",
                        ));
                    }
                    None => return Err(self.ended()),
                }
            }
            b'"' if !started => {}
            _ => {
                return Err(self.document_error(
                    "",
                    Problem::InvalidFormat,
                    &format!(
                        "is not valid JSON: expected a member near byte {}",
                        self.source.offset()
                    ),
                ));
            }
        }
        self.phase = Phase::Members { started: true };
        let offset = self.source.offset();
        let Value::String(key) = self.read_json(KEY_BYTES, Place::Document).await? else {
            return Err(self.syntax_error(Place::Document, offset));
        };
        self.expect(b':', "a colon").await?;
        if self.peek().await?.is_none() {
            return Err(self.ended());
        }
        Ok(Some(key))
    }

    async fn json_header(&mut self) -> Result<ImportHeader, ImportError> {
        if self.peek().await?.is_none() {
            return Err(self.document_error("", Problem::Required, "is empty"));
        }
        if self.source.pending()[0] != b'{' {
            return Err(self.document_error("", Problem::InvalidFormat, "must be a JSON object"));
        }
        self.source.consume(1);
        let mut builder = HeaderBuilder::default();
        loop {
            let Some(key) = self.member_key().await? else {
                builder.finish(Place::Document)?;
                return Err(self.document_error(COLLECTION_KEY, Problem::Required, "is required"));
            };
            if key == COLLECTION_KEY {
                let header = std::mem::take(&mut builder).finish(Place::Document)?;
                self.expect(b'[', "the capture list").await?;
                self.phase = Phase::Captures { first: true };
                return Ok(header);
            }
            let value = self.read_json(MEMBER_VALUE_BYTES, Place::Document).await?;
            builder.set(Place::Document, &key, value)?;
        }
    }

    async fn json_capture(&mut self) -> Result<Option<ImportCapture>, ImportError> {
        loop {
            match self.phase {
                Phase::Captures { first } => {
                    let Some(next) = self.peek().await? else {
                        return Err(self.ended());
                    };
                    match (first, next) {
                        (_, b']') => {
                            self.source.consume(1);
                            self.phase = Phase::AfterCaptures;
                            continue;
                        }
                        (false, b',') => {
                            self.source.consume(1);
                            if self.peek().await?.is_none() {
                                return Err(self.ended());
                            }
                        }
                        (true, _) => {}
                        (false, _) => {
                            return Err(self.document_error(
                                "",
                                Problem::InvalidFormat,
                                &format!(
                                    "is not valid JSON: expected a comma near byte {}",
                                    self.source.offset()
                                ),
                            ));
                        }
                    }
                    let place = Place::Capture(self.index);
                    if self.source.pending()[0] != b'{' {
                        return Err(ImportError::invalid(
                            place,
                            "",
                            Problem::InvalidFormat,
                            "must be an object",
                        ));
                    }
                    let value = self.read_json(self.limits.element_bytes(), place).await?;
                    self.phase = Phase::Captures { first: false };
                    self.index += 1;
                    return capture::parse_capture(value, place, &self.limits).map(Some);
                }
                Phase::AfterCaptures => {
                    self.phase = Phase::Members { started: true };
                }
                Phase::Members { .. } => {
                    if let Some(key) = self.member_key().await? {
                        let shown: String = key.chars().take(64).collect();
                        return Err(self.document_error(
                            &shown,
                            Problem::NotAllowed,
                            "is not allowed after the captures",
                        ));
                    }
                }
                Phase::Done => {
                    if self.peek().await?.is_some() {
                        return Err(self.document_error(
                            "",
                            Problem::InvalidFormat,
                            "has data after the end of the document",
                        ));
                    }
                    return Ok(None);
                }
                Phase::Lines => return Ok(None),
            }
        }
    }

    async fn next_value_line(&mut self) -> Result<Option<(Place, Value)>, ImportError> {
        let cap = self.limits.element_bytes();
        let mut skipped = 0_usize;
        loop {
            let pending = self.source.pending();
            let from = self.scanned.min(pending.len());
            let found = pending[from..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|offset| from + offset);
            let (end, consumed) = match found {
                Some(end) => (end, end + 1),
                None => {
                    self.scanned = pending.len();
                    if pending.len() > cap {
                        return Err(too_long(Place::Line(self.line + 1), cap));
                    }
                    if self.source.fill().await? {
                        continue;
                    }
                    let rest = self.source.pending().len();
                    if rest == 0 {
                        return Ok(None);
                    }
                    (rest, rest)
                }
            };
            if end > cap {
                return Err(too_long(Place::Line(self.line + 1), cap));
            }
            self.line += 1;
            let place = Place::Line(self.line);
            let offset = self.source.offset();
            let mut line = &self.source.pending()[..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            let parsed = if line.iter().all(|byte| is_space(*byte)) {
                None
            } else {
                Some(serde_json::from_slice::<Value>(line))
            };
            self.source.consume(consumed);
            self.scanned = 0;
            match parsed {
                None => {
                    skipped += 1;
                    if skipped.is_multiple_of(BLANK_LINES_PER_YIELD) {
                        tokio::task::yield_now().await;
                    }
                }
                Some(Ok(value)) => return Ok(Some((place, value))),
                Some(Err(_)) => return Err(self.syntax_error(place, offset)),
            }
        }
    }

    async fn ndjson_header(&mut self) -> Result<ImportHeader, ImportError> {
        let Some((place, value)) = self.next_value_line().await? else {
            return Err(self.document_error("", Problem::Required, "is empty"));
        };
        let Value::Object(map) = value else {
            return Err(ImportError::invalid(
                place,
                "",
                Problem::InvalidFormat,
                "must be a JSON object",
            ));
        };
        let mut builder = HeaderBuilder::default();
        for (key, value) in map {
            builder.set(place, &key, value)?;
        }
        builder.finish(place)
    }

    async fn ndjson_capture(&mut self) -> Result<Option<ImportCapture>, ImportError> {
        let Some((place, value)) = self.next_value_line().await? else {
            self.phase = Phase::Done;
            return Ok(None);
        };
        capture::parse_capture(value, place, &self.limits).map(Some)
    }
}
