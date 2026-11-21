use crate::ast::{
    Arg, CompiledJsonPath, Expression, Filter, FilterKind, Function, JsonSource, Key, Part,
    Primary, Reference, Segment, StepField, Template,
};
use crate::error::{Diagnostic, ErrorCode, TemplateError};
use crate::format;
use crate::jsonpath;
use crate::limits::{MAX_ARGUMENTS, MAX_IDENTIFIER_BYTES, MAX_KEY_BYTES};
use crate::span::{ByteRange, locate};
use crate::target::{Policy, SecretPolicy, StepPolicy};

const OPEN: &str = "{{";
const CLOSE: &str = "}}";

struct Failed;

type Syntax<T> = Result<T, Failed>;

enum RawSegment {
    Key(String),
    Index(usize),
}

struct RawSegmentAt {
    segment: RawSegment,
    range: ByteRange,
}

struct RawPath {
    root: String,
    root_range: ByteRange,
    segments: Vec<RawSegmentAt>,
    range: ByteRange,
}

enum RawArgKind {
    Text(String),
    Int(i64),
    Path(RawPath),
}

struct RawArg {
    kind: RawArgKind,
    range: ByteRange,
}

struct Parser<'a> {
    source: &'a str,
    policy: &'a Policy,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    truncated: bool,
    stopped: bool,
    expressions: usize,
    literal_bytes: usize,
    literal_overflow_reported: bool,
}

pub(crate) fn parse(source: &str, policy: &Policy) -> Result<Template, TemplateError> {
    let mut parser = Parser {
        source,
        policy,
        pos: 0,
        diagnostics: Vec::new(),
        truncated: false,
        stopped: false,
        expressions: 0,
        literal_bytes: 0,
        literal_overflow_reported: false,
    };
    let parts = parser.run();
    if parser.diagnostics.is_empty() {
        Ok(Template::assemble(
            source,
            parts,
            policy.target,
            parser.expressions,
        ))
    } else {
        Err(TemplateError {
            diagnostics: parser.diagnostics,
            truncated: parser.truncated,
        })
    }
}

fn is_ident_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_ident_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn is_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '-'
}

impl<'a> Parser<'a> {
    fn report(&mut self, code: ErrorCode, message: impl Into<String>, range: ByteRange) {
        if self.diagnostics.len() >= self.policy.limits.max_diagnostics {
            self.truncated = true;
            self.stopped = true;
            return;
        }
        self.diagnostics.push(Diagnostic {
            code,
            message: message.into(),
            position: locate(self.source, range),
        });
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos..)?.chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.pos += character.len_utf8();
        Some(character)
    }

    fn starts_with(&self, text: &str) -> bool {
        self.source
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(text))
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
            self.pos += 1;
        }
    }

    fn unexpected(&mut self, open: ByteRange) -> Failed {
        match self.peek() {
            None => self.report(
                ErrorCode::UnterminatedExpression,
                "expression is not closed with }}",
                open,
            ),
            Some(character) => {
                let range = ByteRange::new(self.pos, self.pos + character.len_utf8());
                self.report(
                    ErrorCode::UnexpectedCharacter,
                    format!("unexpected character {character:?}"),
                    range,
                );
            }
        }
        Failed
    }

    fn run(&mut self) -> Vec<Part> {
        let mut parts = Vec::new();
        if self.source.len() > self.policy.limits.max_source_bytes {
            self.report(
                ErrorCode::SourceTooLarge,
                format!(
                    "template is larger than {} bytes",
                    self.policy.limits.max_source_bytes
                ),
                ByteRange::new(0, 0),
            );
            return parts;
        }
        let mut cursor = 0;
        loop {
            let rest = self.source.get(cursor..).unwrap_or("");
            let Some(relative) = rest.find(OPEN) else {
                self.push_text(&mut parts, cursor, self.source.len());
                break;
            };
            let open = cursor + relative;
            self.push_text(&mut parts, cursor, open);
            if self.stopped {
                break;
            }
            self.pos = open + OPEN.len();
            match self.expression(open) {
                Ok(expression) => {
                    parts.push(Part::Expression(expression));
                    cursor = self.pos;
                }
                Err(Failed) => {
                    if self.stopped {
                        break;
                    }
                    cursor = self.resynchronise(open);
                }
            }
            if self.stopped {
                break;
            }
        }
        parts
    }

    fn resynchronise(&self, open: usize) -> usize {
        let from = self.pos.max(open + OPEN.len());
        match self.source.get(from..).and_then(|rest| rest.find(CLOSE)) {
            Some(relative) => from + relative + CLOSE.len(),
            None => self.source.len(),
        }
    }

    fn push_text(&mut self, parts: &mut Vec<Part>, from: usize, to: usize) {
        if to <= from {
            return;
        }
        let Some(text) = self.source.get(from..to) else {
            return;
        };
        if let Some((index, character)) = self.policy.target.first_forbidden(text) {
            let start = from + index;
            self.report(
                ErrorCode::ForbiddenCharacter,
                format!("character {character:?} is not allowed in this output"),
                ByteRange::new(start, start + character.len_utf8()),
            );
        }
        self.add_literal_bytes(text.len(), ByteRange::new(from, to));
        parts.push(Part::Text(ByteRange::new(from, to)));
    }

    fn add_literal_bytes(&mut self, bytes: usize, range: ByteRange) {
        self.literal_bytes = self.literal_bytes.saturating_add(bytes);
        let limit = self.policy.target.default_output_limit();
        if self.literal_bytes > limit && !self.literal_overflow_reported {
            self.literal_overflow_reported = true;
            self.report(
                ErrorCode::OutputTooLarge,
                format!("literal text alone exceeds the output limit of {limit} bytes"),
                range,
            );
        }
    }

    fn expression(&mut self, open: usize) -> Syntax<Expression> {
        let open_range = ByteRange::new(open, open + OPEN.len());
        self.expressions += 1;
        if self.expressions > self.policy.limits.max_expressions {
            self.report(
                ErrorCode::TooManyExpressions,
                format!(
                    "template has more than {} expressions",
                    self.policy.limits.max_expressions
                ),
                open_range,
            );
            self.stopped = true;
            return Err(Failed);
        }
        self.skip_whitespace();
        let primary_start = self.pos;
        let primary = self.primary(open_range)?;
        let primary_range = ByteRange::new(primary_start, self.pos);
        let mut filters = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek() != Some('|') {
                break;
            }
            self.pos += 1;
            self.skip_whitespace();
            let filter = self.filter(open_range)?;
            if filters.len() == self.policy.limits.max_filters_per_expression {
                self.report(
                    ErrorCode::TooManyFilters,
                    format!(
                        "expression has more than {} filters",
                        self.policy.limits.max_filters_per_expression
                    ),
                    filter.range,
                );
            }
            filters.push(filter);
            if self.stopped {
                return Err(Failed);
            }
        }
        self.skip_whitespace();
        if self.starts_with(CLOSE) {
            self.pos += CLOSE.len();
        } else {
            return Err(self.unexpected(open_range));
        }
        if filters.is_empty() {
            self.check_literal_primary(&primary, primary_range);
        }
        Ok(Expression {
            range: ByteRange::new(open, self.pos),
            primary_range,
            primary,
            filters,
        })
    }

    fn check_literal_primary(&mut self, primary: &Primary, range: ByteRange) {
        let text = match primary {
            Primary::Text(text) => text.clone(),
            Primary::Int(number) => number.to_string(),
            _ => return,
        };
        if let Some((_, character)) = self.policy.target.first_forbidden(&text) {
            self.report(
                ErrorCode::ForbiddenCharacter,
                format!("character {character:?} is not allowed in this output"),
                range,
            );
        }
        self.add_literal_bytes(text.len(), range);
    }

    fn primary(&mut self, open: ByteRange) -> Syntax<Primary> {
        match self.peek() {
            None => Err(self.unexpected(open)),
            Some('"') => {
                let (text, _) = self.string()?;
                Ok(Primary::Text(text))
            }
            Some(character) if character.is_ascii_digit() || character == '-' => {
                let (number, _) = self.integer()?;
                Ok(Primary::Int(number))
            }
            Some(character) if is_ident_start(character) => {
                let (name, name_range) = self.identifier()?;
                if self.peek() == Some('(') {
                    self.pos += 1;
                    let args = self.arguments(open, true)?;
                    let range = ByteRange::new(name_range.start, self.pos);
                    Ok(Primary::Function(
                        self.resolve_function(&name, name_range, args, range),
                    ))
                } else {
                    let path = self.path_tail(name, name_range, open)?;
                    Ok(Primary::Reference(self.resolve_reference(&path)))
                }
            }
            Some(_) => Err(self.unexpected(open)),
        }
    }

    fn identifier(&mut self) -> Syntax<(String, ByteRange)> {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        let range = ByteRange::new(start, self.pos);
        if range.end - range.start > MAX_IDENTIFIER_BYTES {
            self.report(
                ErrorCode::LiteralTooLarge,
                format!("identifier is longer than {MAX_IDENTIFIER_BYTES} bytes"),
                range,
            );
            return Err(Failed);
        }
        let name = self.source.get(start..self.pos).unwrap_or("").to_owned();
        Ok((name, range))
    }

    fn string(&mut self) -> Syntax<(String, ByteRange)> {
        let start = self.pos;
        self.pos += 1;
        let mut text = String::new();
        loop {
            let character_start = self.pos;
            match self.bump() {
                None | Some('\n') => {
                    self.report(
                        ErrorCode::UnterminatedString,
                        "string literal is not closed",
                        ByteRange::new(start, self.pos),
                    );
                    self.pos = start + 1;
                    return Err(Failed);
                }
                Some('"') => break,
                Some('\\') => {
                    let escaped = match self.bump() {
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('\\') => '\\',
                        Some('"') => '"',
                        Some(other) => {
                            self.report(
                                ErrorCode::InvalidEscape,
                                format!("unknown escape sequence \\{other}"),
                                ByteRange::new(character_start, self.pos),
                            );
                            return Err(Failed);
                        }
                        None => {
                            self.report(
                                ErrorCode::UnterminatedString,
                                "string literal is not closed",
                                ByteRange::new(start, self.pos),
                            );
                            self.pos = start + 1;
                            return Err(Failed);
                        }
                    };
                    text.push(escaped);
                }
                Some(character) => text.push(character),
            }
            if text.len() > self.policy.limits.max_literal_bytes {
                self.report(
                    ErrorCode::LiteralTooLarge,
                    format!(
                        "string literal is longer than {} bytes",
                        self.policy.limits.max_literal_bytes
                    ),
                    ByteRange::new(start, self.pos),
                );
                self.pos = start + 1;
                return Err(Failed);
            }
        }
        Ok((text, ByteRange::new(start, self.pos)))
    }

    fn integer(&mut self) -> Syntax<(i64, ByteRange)> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        let digits_start = self.pos;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.pos += 1;
        }
        let range = ByteRange::new(start, self.pos);
        let digits = self.pos - digits_start;
        if digits == 0 || digits > 18 {
            self.report(
                ErrorCode::InvalidNumber,
                "integer literal needs between 1 and 18 digits",
                range,
            );
            return Err(Failed);
        }
        match self.source.get(start..self.pos).map(str::parse::<i64>) {
            Some(Ok(number)) => Ok((number, range)),
            _ => {
                self.report(ErrorCode::InvalidNumber, "invalid integer literal", range);
                Err(Failed)
            }
        }
    }

    fn path_tail(
        &mut self,
        root: String,
        root_range: ByteRange,
        open: ByteRange,
    ) -> Syntax<RawPath> {
        let mut segments: Vec<RawSegmentAt> = Vec::new();
        let max_segments = self.policy.limits.max_path_segments;
        loop {
            let start = self.pos;
            let segment = match self.peek() {
                Some('.') => {
                    self.pos += 1;
                    let key_start = self.pos;
                    while self.peek().is_some_and(is_key_character) {
                        self.pos += 1;
                    }
                    if self.pos == key_start {
                        return Err(self.unexpected(open));
                    }
                    if self.pos - key_start > MAX_KEY_BYTES {
                        self.report(
                            ErrorCode::LiteralTooLarge,
                            format!("key is longer than {MAX_KEY_BYTES} bytes"),
                            ByteRange::new(start, self.pos),
                        );
                        return Err(Failed);
                    }
                    let key = self
                        .source
                        .get(key_start..self.pos)
                        .unwrap_or("")
                        .to_owned();
                    RawSegment::Key(key)
                }
                Some('[') => {
                    self.pos += 1;
                    self.skip_whitespace();
                    let segment = match self.peek() {
                        Some('"') => {
                            let (text, _) = self.string()?;
                            RawSegment::Key(text)
                        }
                        Some('-') => {
                            self.report(
                                ErrorCode::InvalidNumber,
                                "index must not be negative",
                                ByteRange::new(self.pos, self.pos + 1),
                            );
                            return Err(Failed);
                        }
                        Some(character) if character.is_ascii_digit() => {
                            let (number, range) = self.integer()?;
                            match usize::try_from(number) {
                                Ok(index) => RawSegment::Index(index),
                                Err(_) => {
                                    self.report(ErrorCode::InvalidNumber, "invalid index", range);
                                    return Err(Failed);
                                }
                            }
                        }
                        _ => return Err(self.unexpected(open)),
                    };
                    self.skip_whitespace();
                    if self.peek() == Some(']') {
                        self.pos += 1;
                    } else {
                        return Err(self.unexpected(open));
                    }
                    segment
                }
                _ => break,
            };
            let range = ByteRange::new(start, self.pos);
            if segments.len() == max_segments {
                self.report(
                    ErrorCode::PathTooDeep,
                    format!("path has more than {max_segments} segments"),
                    range,
                );
                if self.stopped {
                    return Err(Failed);
                }
            }
            segments.push(RawSegmentAt { segment, range });
        }
        Ok(RawPath {
            range: ByteRange::new(root_range.start, self.pos),
            root,
            root_range,
            segments,
        })
    }

    fn arguments(&mut self, open: ByteRange, allow_empty: bool) -> Syntax<Vec<RawArg>> {
        let mut args = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(')') {
            if allow_empty {
                self.pos += 1;
                return Ok(args);
            }
            return Err(self.unexpected(open));
        }
        loop {
            self.skip_whitespace();
            let start = self.pos;
            let kind = match self.peek() {
                Some('"') => RawArgKind::Text(self.string()?.0),
                Some(character) if character.is_ascii_digit() || character == '-' => {
                    RawArgKind::Int(self.integer()?.0)
                }
                Some(character) if is_ident_start(character) => {
                    let (name, name_range) = self.identifier()?;
                    RawArgKind::Path(self.path_tail(name, name_range, open)?)
                }
                _ => return Err(self.unexpected(open)),
            };
            args.push(RawArg {
                kind,
                range: ByteRange::new(start, self.pos),
            });
            if args.len() > MAX_ARGUMENTS {
                self.report(
                    ErrorCode::WrongArgumentCount,
                    format!("at most {MAX_ARGUMENTS} arguments are allowed"),
                    ByteRange::new(start, self.pos),
                );
                return Err(Failed);
            }
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(')') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.unexpected(open)),
            }
        }
        Ok(args)
    }

    fn filter(&mut self, open: ByteRange) -> Syntax<Filter> {
        if !self.peek().is_some_and(is_ident_start) {
            return Err(self.unexpected(open));
        }
        let (name, name_range) = self.identifier()?;
        let args = if self.peek() == Some('(') {
            self.pos += 1;
            self.arguments(open, false)?
        } else {
            Vec::new()
        };
        let range = ByteRange::new(name_range.start, self.pos);
        let kind = self.resolve_filter(&name, name_range, args, range);
        Ok(Filter { kind, range })
    }

    fn check_arity(
        &mut self,
        args: &[RawArg],
        expected: usize,
        name: &str,
        range: ByteRange,
    ) -> bool {
        if args.len() == expected {
            return true;
        }
        self.report(
            ErrorCode::WrongArgumentCount,
            format!("{name} takes {expected} argument(s), found {}", args.len()),
            range,
        );
        false
    }

    fn any_arg(&mut self, raw: RawArg) -> Arg {
        match raw.kind {
            RawArgKind::Text(text) => Arg::Text(text),
            RawArgKind::Int(number) => Arg::Int(number),
            RawArgKind::Path(path) => Arg::Reference(self.resolve_reference(&path)),
        }
    }

    fn text_arg(&mut self, raw: RawArg) -> Arg {
        if matches!(raw.kind, RawArgKind::Int(_)) {
            self.report(
                ErrorCode::WrongArgumentType,
                "expected a string or a variable",
                raw.range,
            );
            return Arg::Text(String::new());
        }
        self.any_arg(raw)
    }

    fn int_arg(&mut self, raw: RawArg) -> Arg {
        if matches!(raw.kind, RawArgKind::Text(_)) {
            self.report(
                ErrorCode::WrongArgumentType,
                "expected an integer or a variable",
                raw.range,
            );
            return Arg::Int(0);
        }
        self.any_arg(raw)
    }

    fn resolve_filter(
        &mut self,
        name: &str,
        name_range: ByteRange,
        args: Vec<RawArg>,
        range: ByteRange,
    ) -> FilterKind {
        let expected = match name {
            "upper" | "lower" | "trim" | "length" | "json" | "url" | "html" | "base64"
            | "base64_decode" | "hex" | "sha256" => 0,
            "default" | "truncate" | "hmac_sha256" | "date" => 1,
            "replace" => 2,
            _ => {
                self.report(
                    ErrorCode::UnknownFilter,
                    format!("unknown filter {name}"),
                    name_range,
                );
                return FilterKind::Upper;
            }
        };
        if !self.check_arity(&args, expected, name, range) {
            return FilterKind::Upper;
        }
        let mut args = args.into_iter();
        let first = args.next();
        let second = args.next();
        match (name, first, second) {
            ("default", Some(raw), _) => FilterKind::Default(self.any_arg(raw)),
            ("truncate", Some(raw), _) => {
                if let RawArgKind::Int(number) = raw.kind
                    && number < 0
                {
                    self.report(
                        ErrorCode::ArgumentOutOfRange,
                        "truncate length must not be negative",
                        raw.range,
                    );
                }
                FilterKind::Truncate(self.int_arg(raw))
            }
            ("replace", Some(from), Some(to)) => {
                if let RawArgKind::Text(text) = &from.kind
                    && text.is_empty()
                {
                    self.report(
                        ErrorCode::ArgumentOutOfRange,
                        "replace needs a non-empty search string",
                        from.range,
                    );
                }
                let from = self.text_arg(from);
                let to = self.text_arg(to);
                FilterKind::Replace(from, to)
            }
            ("hmac_sha256", Some(raw), _) => FilterKind::HmacSha256(self.text_arg(raw)),
            ("date", Some(raw), _) => {
                if let RawArgKind::Text(text) = &raw.kind
                    && let Err(message) = format::validate(text)
                {
                    self.report(ErrorCode::InvalidDateFormat, message, raw.range);
                }
                FilterKind::Date(self.text_arg(raw))
            }
            ("upper", ..) => FilterKind::Upper,
            ("lower", ..) => FilterKind::Lower,
            ("trim", ..) => FilterKind::Trim,
            ("length", ..) => FilterKind::Length,
            ("json", ..) => FilterKind::Json,
            ("url", ..) => FilterKind::Url,
            ("html", ..) => FilterKind::Html,
            ("base64", ..) => FilterKind::Base64,
            ("base64_decode", ..) => FilterKind::Base64Decode,
            ("hex", ..) => FilterKind::Hex,
            ("sha256", ..) => FilterKind::Sha256,
            _ => FilterKind::Upper,
        }
    }

    fn compile_json_path(&mut self, raw: &RawArg) -> CompiledJsonPath {
        let RawArgKind::Text(text) = &raw.kind else {
            self.report(
                ErrorCode::WrongArgumentType,
                "the JSON path must be a string literal",
                raw.range,
            );
            return Self::placeholder_path();
        };
        match jsonpath::compile(text) {
            Ok(path) => CompiledJsonPath { path },
            Err(problem) => {
                self.report(problem.code, problem.message, raw.range);
                Self::placeholder_path()
            }
        }
    }

    fn placeholder_path() -> CompiledJsonPath {
        CompiledJsonPath {
            path: serde_json_path::JsonPath::default(),
        }
    }

    fn resolve_function(
        &mut self,
        name: &str,
        name_range: ByteRange,
        args: Vec<RawArg>,
        range: ByteRange,
    ) -> Function {
        match name {
            "uuid" => {
                self.check_arity(&args, 0, name, range);
                Function::Uuid
            }
            "random_hex" => {
                if !self.check_arity(&args, 1, name, range) {
                    return Function::Uuid;
                }
                let mut args = args.into_iter();
                let Some(raw) = args.next() else {
                    return Function::Uuid;
                };
                if let RawArgKind::Int(number) = raw.kind
                    && !(1..=64).contains(&number)
                {
                    self.report(
                        ErrorCode::ArgumentOutOfRange,
                        "random_hex length must be between 1 and 64",
                        raw.range,
                    );
                }
                Function::RandomHex(self.int_arg(raw))
            }
            "random_int" => {
                if !self.check_arity(&args, 2, name, range) {
                    return Function::Uuid;
                }
                let mut args = args.into_iter();
                let (Some(low), Some(high)) = (args.next(), args.next()) else {
                    return Function::Uuid;
                };
                if let (RawArgKind::Int(low_value), RawArgKind::Int(high_value)) =
                    (&low.kind, &high.kind)
                    && low_value > high_value
                {
                    self.report(
                        ErrorCode::ArgumentOutOfRange,
                        "random_int lower bound is above the upper bound",
                        range,
                    );
                }
                let low = self.int_arg(low);
                let high = self.int_arg(high);
                Function::RandomInt(low, high)
            }
            "json" => self.resolve_json(args, range),
            _ => {
                self.report(
                    ErrorCode::UnknownFunction,
                    format!("unknown function {name}"),
                    name_range,
                );
                Function::Uuid
            }
        }
    }

    fn resolve_json(&mut self, args: Vec<RawArg>, range: ByteRange) -> Function {
        match args.len() {
            1 => {
                let path = self.compile_json_path(&args[0]);
                Function::Json(JsonSource::Request, path)
            }
            2 => {
                let path = self.compile_json_path(&args[1]);
                let source = match &args[0].kind {
                    RawArgKind::Path(raw_path) => match self.resolve_reference(raw_path) {
                        Reference::Step(name, StepField::Body) => JsonSource::Step(name),
                        _ => {
                            self.report(
                                ErrorCode::WrongArgumentType,
                                "the first argument of json must be step.<name>.body",
                                args[0].range,
                            );
                            JsonSource::Request
                        }
                    },
                    _ => {
                        self.report(
                            ErrorCode::WrongArgumentType,
                            "the first argument of json must be step.<name>.body",
                            args[0].range,
                        );
                        JsonSource::Request
                    }
                };
                Function::Json(source, path)
            }
            other => {
                self.report(
                    ErrorCode::WrongArgumentCount,
                    format!("json takes 1 or 2 arguments, found {other}"),
                    range,
                );
                Function::Uuid
            }
        }
    }

    fn resolve_reference(&mut self, path: &RawPath) -> Reference {
        let fallback = Reference::Now;
        match path.root.as_str() {
            "request" => self.resolve_request(path).unwrap_or(fallback),
            "capture" => self
                .single_field(path, &["id", "seq", "received_at"])
                .map(|field| match field {
                    "id" => Reference::CaptureId,
                    "seq" => Reference::CaptureSeq,
                    _ => Reference::CaptureReceivedAt,
                })
                .unwrap_or(fallback),
            "endpoint" => self
                .single_field(path, &["id", "url", "title"])
                .map(|field| match field {
                    "id" => Reference::EndpointId,
                    "url" => Reference::EndpointUrl,
                    _ => Reference::EndpointTitle,
                })
                .unwrap_or(fallback),
            "now" => {
                if let Some(extra) = path.segments.first() {
                    self.report(
                        ErrorCode::UnexpectedSegment,
                        "now has no fields",
                        extra.range,
                    );
                }
                Reference::Now
            }
            "var" | "store" => self.resolve_named_value(path).unwrap_or(fallback),
            "counter" => self
                .named_key(path, "counter")
                .map(Reference::Counter)
                .unwrap_or(fallback),
            "step" => self.resolve_step(path).unwrap_or(fallback),
            "secret" => self.resolve_secret(path).unwrap_or(fallback),
            other => {
                self.report(
                    ErrorCode::UnknownRoot,
                    format!("unknown root {other}"),
                    path.root_range,
                );
                fallback
            }
        }
    }

    fn field_name<'p>(&mut self, path: &'p RawPath, index: usize, what: &str) -> Option<&'p str> {
        let Some(segment) = path.segments.get(index) else {
            self.report(
                ErrorCode::MissingKey,
                format!("{what} needs a field name"),
                path.range,
            );
            return None;
        };
        match &segment.segment {
            RawSegment::Key(key) => Some(key.as_str()),
            RawSegment::Index(_) => {
                self.report(
                    ErrorCode::InvalidSegment,
                    "expected a name here, not an index",
                    segment.range,
                );
                None
            }
        }
    }

    fn no_more_segments(&mut self, path: &RawPath, from: usize) -> bool {
        match path.segments.get(from) {
            Some(extra) => {
                self.report(
                    ErrorCode::UnexpectedSegment,
                    "this value has no further fields",
                    extra.range,
                );
                false
            }
            None => true,
        }
    }

    fn single_field(&mut self, path: &RawPath, allowed: &[&'static str]) -> Option<&'static str> {
        let name = self.field_name(path, 0, &path.root)?.to_owned();
        let Some(found) = allowed.iter().find(|candidate| **candidate == name) else {
            let range = path.segments.first().map_or(path.range, |s| s.range);
            self.report(
                ErrorCode::UnknownField,
                format!("{} has no field {name}", path.root),
                range,
            );
            return None;
        };
        self.no_more_segments(path, 1).then_some(*found)
    }

    fn named_key(&mut self, path: &RawPath, what: &str) -> Option<Key> {
        let name = self.field_name(path, 0, what)?.to_owned();
        self.no_more_segments(path, 1).then(|| Key::new(name))
    }

    fn keyed(&mut self, path: &RawPath, index: usize, what: &str) -> Option<Key> {
        let name = self.field_name(path, index, what)?.to_owned();
        self.no_more_segments(path, index + 1)
            .then(|| Key::new(name))
    }

    fn resolve_request(&mut self, path: &RawPath) -> Option<Reference> {
        let field = self.field_name(path, 0, "request")?.to_owned();
        let range = path.segments.first().map_or(path.range, |s| s.range);
        let simple = |reference: Reference, parser: &mut Self| {
            parser.no_more_segments(path, 1).then_some(reference)
        };
        match field.as_str() {
            "method" => simple(Reference::RequestMethod, self),
            "path" => simple(Reference::RequestPath, self),
            "headers" => simple(Reference::RequestHeaders, self),
            "body" => simple(Reference::RequestBody, self),
            "ip" => simple(Reference::RequestIp, self),
            "content_type" => simple(Reference::RequestContentType, self),
            "kind" => simple(Reference::RequestKind, self),
            "signature" => simple(Reference::RequestSignature, self),
            "segments" => {
                let Some(segment) = path.segments.get(1) else {
                    self.report(ErrorCode::MissingKey, "segments needs an index", path.range);
                    return None;
                };
                let RawSegment::Index(index) = segment.segment else {
                    self.report(
                        ErrorCode::InvalidSegment,
                        "segments expects an index",
                        segment.range,
                    );
                    return None;
                };
                self.no_more_segments(path, 2)
                    .then_some(Reference::RequestSegment(index))
            }
            "query" => self.keyed(path, 1, "query").map(Reference::RequestQuery),
            "form" => self.keyed(path, 1, "form").map(Reference::RequestForm),
            "meta" => self.keyed(path, 1, "meta").map(Reference::RequestMeta),
            "header" => self
                .keyed(path, 1, "header")
                .map(|key| Reference::RequestHeader(Key::new(key.unpack().to_ascii_lowercase()))),
            other => {
                self.report(
                    ErrorCode::UnknownField,
                    format!("request has no field {other}"),
                    range,
                );
                None
            }
        }
    }

    fn resolve_named_value(&mut self, path: &RawPath) -> Option<Reference> {
        let name = Key::new(self.field_name(path, 0, &path.root)?);
        let tail = path
            .segments
            .iter()
            .skip(1)
            .map(|item| match &item.segment {
                RawSegment::Key(key) => Segment::Key(Key::new(key.clone())),
                RawSegment::Index(index) => Segment::Index(*index),
            })
            .collect();
        if path.root == "var" {
            Some(Reference::Var(name, tail))
        } else {
            Some(Reference::Store(name, tail))
        }
    }

    fn resolve_step(&mut self, path: &RawPath) -> Option<Reference> {
        let name = self.field_name(path, 0, "step")?.to_owned();
        if let StepPolicy::Only(known) = &self.policy.steps
            && !known.contains(&name)
        {
            let range = path.segments.first().map_or(path.range, |s| s.range);
            self.report(
                ErrorCode::StepUnknown,
                format!("no earlier step is named {name}"),
                range,
            );
        }
        let field = self.field_name(path, 1, "step.<name>")?.to_owned();
        let range = path.segments.get(1).map_or(path.range, |s| s.range);
        let step = Key::new(name);
        let simple = |field: StepField, parser: &mut Self| {
            parser
                .no_more_segments(path, 2)
                .then(|| Reference::Step(step.clone(), field))
        };
        match field.as_str() {
            "status" => simple(StepField::Status, self),
            "body" => simple(StepField::Body, self),
            "duration_ms" => simple(StepField::DurationMs, self),
            "error" => simple(StepField::Error, self),
            "header" => {
                let key = self.keyed(path, 2, "step.<name>.header")?;
                Some(Reference::Step(
                    step,
                    StepField::Header(Key::new(key.unpack().to_ascii_lowercase())),
                ))
            }
            other => {
                self.report(
                    ErrorCode::UnknownField,
                    format!("step has no field {other}"),
                    range,
                );
                None
            }
        }
    }

    fn resolve_secret(&mut self, path: &RawPath) -> Option<Reference> {
        if self.policy.secrets == SecretPolicy::Forbidden {
            self.report(
                ErrorCode::SecretNotAllowed,
                "secrets are not allowed in this template",
                path.range,
            );
            return None;
        }
        self.named_key(path, "secret").map(Reference::Secret)
    }
}
