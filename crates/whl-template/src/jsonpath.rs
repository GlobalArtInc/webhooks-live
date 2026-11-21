use serde_json_path::JsonPath;

use crate::error::ErrorCode;

const MAX_PATH_BYTES: usize = 256;
const MAX_DESCENDANTS: usize = 1;
const MAX_FILTERS: usize = 1;
const MAX_NESTING: usize = 8;
const MAX_UNION_SELECTORS: usize = 3;
const MAX_MULTIPLYING_AFTER_FIRST: usize = 2;
const MAX_OPERATORS: usize = 4;
const MAX_FILTER_WILDCARDS: usize = 1;
const MAX_MESSAGE_BYTES: usize = 200;

pub(crate) struct PathProblem {
    pub code: ErrorCode,
    pub message: String,
}

fn too_complex(message: &str) -> PathProblem {
    PathProblem {
        code: ErrorCode::JsonPathTooComplex,
        message: message.to_owned(),
    }
}

fn invalid(message: &str) -> PathProblem {
    PathProblem {
        code: ErrorCode::InvalidJsonPath,
        message: message.to_owned(),
    }
}

fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_part(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

struct Scanner<'a> {
    bytes: &'a [u8],
    index: usize,
    segments: usize,
    multiplying_after_first: usize,
    union_selectors: usize,
    descendants: usize,
    filters: usize,
    operators: usize,
    filter_wildcards: usize,
}

struct Bracket {
    multiplies: bool,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            index: 0,
            segments: 0,
            multiplying_after_first: 0,
            union_selectors: 0,
            descendants: 0,
            filters: 0,
            operators: 0,
            filter_wildcards: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.index + offset).copied()
    }

    fn skip_blank(&mut self) {
        while self.peek().is_some_and(is_blank) {
            self.index += 1;
        }
    }

    fn open_segment(&mut self, multiplies: bool) -> Result<(), PathProblem> {
        self.segments += 1;
        if multiplies && self.segments > 1 {
            self.multiplying_after_first += 1;
            if self.multiplying_after_first > MAX_MULTIPLYING_AFTER_FIRST {
                return Err(too_complex(
                    "JSON path may have at most 2 wildcard, union, slice, filter or descendant segments after the first segment",
                ));
            }
        }
        Ok(())
    }

    fn run(&mut self) -> Result<(), PathProblem> {
        self.skip_blank();
        if self.peek() != Some(b'$') {
            return Err(invalid("JSON path must start with $"));
        }
        self.index += 1;
        loop {
            self.skip_blank();
            match self.peek() {
                None => return Ok(()),
                Some(b'.') => self.dotted()?,
                Some(b'[') => {
                    let bracket = self.bracket()?;
                    self.open_segment(bracket.multiplies)?;
                }
                Some(_) => return Err(invalid("unexpected character in JSON path")),
            }
        }
    }

    fn name_run(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| !is_blank(byte) && byte != b'.' && byte != b'[')
        {
            self.index += 1;
        }
    }

    fn dotted(&mut self) -> Result<(), PathProblem> {
        if self.peek_at(1) == Some(b'.') {
            self.index += 2;
            self.descendants += 1;
            if self.descendants > MAX_DESCENDANTS {
                return Err(too_complex(
                    "JSON path may contain at most one descendant segment",
                ));
            }
            match self.peek() {
                Some(b'[') => {
                    self.bracket()?;
                }
                Some(b'*') => self.index += 1,
                _ => self.name_run(),
            }
            return self.open_segment(true);
        }
        self.index += 1;
        if self.peek() == Some(b'*') {
            self.index += 1;
            return self.open_segment(true);
        }
        self.name_run();
        self.open_segment(false)
    }

    fn skip_string(&mut self, quote: u8) -> Result<(), PathProblem> {
        self.index += 1;
        loop {
            match self.peek() {
                None => return Err(invalid("unterminated string in JSON path")),
                Some(b'\\') => self.index += 2,
                Some(byte) if byte == quote => {
                    self.index += 1;
                    return Ok(());
                }
                Some(_) => self.index += 1,
            }
        }
    }

    fn count_operator(&mut self, width: usize) -> Result<(), PathProblem> {
        self.index += width;
        self.operators += 1;
        if self.operators > MAX_OPERATORS {
            return Err(too_complex(
                "JSON path may contain at most 4 comparison or logical operators",
            ));
        }
        Ok(())
    }

    fn check_function(&mut self) -> Result<(), PathProblem> {
        let start = self.index;
        while self.peek().is_some_and(is_identifier_part) {
            self.index += 1;
        }
        let name = self.bytes.get(start..self.index).unwrap_or(&[]);
        let mut ahead = self.index;
        while self.bytes.get(ahead).copied().is_some_and(is_blank) {
            ahead += 1;
        }
        let called = self.bytes.get(ahead) == Some(&b'(');
        if called && (name == b"match" || name == b"search") {
            return Err(invalid(
                "the regular expression functions match and search are not supported",
            ));
        }
        Ok(())
    }

    fn require_literal(&mut self) -> Result<(), PathProblem> {
        self.skip_blank();
        let literal = match self.peek() {
            Some(b'\'' | b'"' | b'-') => true,
            Some(byte) if byte.is_ascii_digit() => true,
            Some(byte) if is_identifier_start(byte) => {
                let mut end = self.index;
                while self.bytes.get(end).copied().is_some_and(is_identifier_part) {
                    end += 1;
                }
                matches!(
                    self.bytes.get(self.index..end),
                    Some(b"true" | b"false" | b"null")
                )
            }
            _ => false,
        };
        if literal {
            Ok(())
        } else {
            Err(too_complex(
                "the right side of a comparison in a JSON path filter must be a literal",
            ))
        }
    }

    fn nesting_check(&self, brackets: usize, parentheses: usize) -> Result<(), PathProblem> {
        if brackets + parentheses > MAX_NESTING {
            return Err(too_complex("JSON path is nested deeper than 8 levels"));
        }
        Ok(())
    }

    fn bracket(&mut self) -> Result<Bracket, PathProblem> {
        self.index += 1;
        let mut brackets = 1usize;
        let mut parentheses = 0usize;
        let mut selectors = 1usize;
        let mut multiplies = false;
        let mut in_filter = false;
        let mut absolute_query = false;
        loop {
            let Some(byte) = self.peek() else {
                return Err(invalid("unterminated bracket in JSON path"));
            };
            match byte {
                b'\'' | b'"' => self.skip_string(byte)?,
                b'[' => {
                    brackets += 1;
                    self.nesting_check(brackets, parentheses)?;
                    self.index += 1;
                }
                b']' => {
                    brackets -= 1;
                    self.index += 1;
                    if brackets == 0 {
                        break;
                    }
                }
                b'(' => {
                    parentheses += 1;
                    self.nesting_check(brackets, parentheses)?;
                    self.index += 1;
                }
                b')' => {
                    parentheses = parentheses.saturating_sub(1);
                    self.index += 1;
                }
                b',' => {
                    if brackets > 1 {
                        return Err(too_complex("JSON path may not use unions inside a filter"));
                    }
                    if parentheses == 0 {
                        selectors += 1;
                        in_filter = false;
                        absolute_query = false;
                    }
                    self.index += 1;
                }
                b'?' => {
                    if in_filter || brackets > 1 {
                        return Err(too_complex("JSON path may not nest filters"));
                    }
                    self.filters += 1;
                    if self.filters > MAX_FILTERS {
                        return Err(too_complex("JSON path may contain at most one filter"));
                    }
                    in_filter = true;
                    multiplies = true;
                    self.index += 1;
                }
                b'*' => {
                    if in_filter {
                        self.filter_wildcards += 1;
                        if self.filter_wildcards > MAX_FILTER_WILDCARDS {
                            return Err(too_complex("a filter may contain at most one wildcard"));
                        }
                        if absolute_query {
                            return Err(too_complex(
                                "a filter may not use a wildcard in a query that starts at $",
                            ));
                        }
                    } else {
                        multiplies = true;
                    }
                    self.index += 1;
                }
                b':' => {
                    if in_filter || brackets > 1 {
                        return Err(too_complex("JSON path may not use slices inside a filter"));
                    }
                    multiplies = true;
                    self.index += 1;
                }
                b'.' => {
                    if self.peek_at(1) == Some(b'.') {
                        return Err(too_complex(
                            "JSON path may not use a descendant segment inside a filter",
                        ));
                    }
                    self.index += 1;
                }
                b'@' => {
                    absolute_query = false;
                    self.index += 1;
                }
                b'$' => {
                    absolute_query = true;
                    self.index += 1;
                }
                b'=' | b'!' | b'<' | b'>' if in_filter => {
                    let width = if self.peek_at(1) == Some(b'=') { 2 } else { 1 };
                    if byte == b'=' && width == 1 {
                        self.index += 1;
                    } else {
                        self.count_operator(width)?;
                        if byte != b'!' || width == 2 {
                            self.require_literal()?;
                        }
                    }
                }
                b'&' | b'|' if in_filter => {
                    let width = if self.peek_at(1) == Some(byte) { 2 } else { 1 };
                    if width == 2 {
                        self.count_operator(width)?;
                    } else {
                        self.index += 1;
                    }
                }
                byte if in_filter && is_identifier_start(byte) => self.check_function()?,
                _ => self.index += 1,
            }
        }
        if selectors > 1 {
            multiplies = true;
            self.union_selectors += selectors;
            if self.union_selectors > MAX_UNION_SELECTORS {
                return Err(too_complex(
                    "JSON path may contain at most 3 comma-separated selectors in total",
                ));
            }
        }
        Ok(Bracket { multiplies })
    }
}

fn preflight(text: &str) -> Result<(), PathProblem> {
    if text.len() > MAX_PATH_BYTES {
        return Err(too_complex("JSON path is longer than 256 bytes"));
    }
    Scanner::new(text).run()
}

pub(crate) fn compile(text: &str) -> Result<JsonPath, PathProblem> {
    preflight(text)?;
    JsonPath::parse(text).map_err(|error| {
        let rendered = error.to_string();
        let first_line = rendered.lines().next().unwrap_or("");
        let message: String = first_line.chars().take(MAX_MESSAGE_BYTES).collect();
        PathProblem {
            code: ErrorCode::InvalidJsonPath,
            message: format!("invalid JSON path: {message}"),
        }
    })
}
