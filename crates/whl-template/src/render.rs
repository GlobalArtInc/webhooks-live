use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value as JsonValue;
use time::OffsetDateTime;
use uuid::Builder;

use crate::ast::{
    Arg, Expression, FilterKind, Function, JsonSource, Key, Part, Primary, Reference, Segment,
    StepField, Template,
};
use crate::context::{Context, parse_json_document};
use crate::env::RenderEnv;
use crate::error::{RenderError, RenderErrorCode};
use crate::filters::{self, FilterFailure};
use crate::format::{self, FormatProblem};
use crate::limits::RenderLimits;
use crate::span::{ByteRange, locate};
use crate::target::RenderTarget;
use crate::value::{Value, compact_json, json_to_value};
use crate::writer::{BoundedWriter, WriteFailure};

const MAX_RANDOM_HEX: i64 = 64;
const MAX_NOTE_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingVariable {
    pub reference: String,
    pub position: crate::span::Position,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub output: String,
    pub missing: Vec<MissingVariable>,
    pub evaluations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialRender {
    pub output: String,
    pub errors: Vec<RenderError>,
    pub missing: Vec<MissingVariable>,
    pub evaluations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderOutcome {
    Clean(Rendered),
    Partial(PartialRender),
}

impl RenderOutcome {
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Clean(_))
    }

    pub fn output(&self) -> &str {
        match self {
            Self::Clean(rendered) => &rendered.output,
            Self::Partial(partial) => &partial.output,
        }
    }

    pub fn errors(&self) -> &[RenderError] {
        match self {
            Self::Clean(_) => &[],
            Self::Partial(partial) => &partial.errors,
        }
    }

    pub fn missing(&self) -> &[MissingVariable] {
        match self {
            Self::Clean(rendered) => &rendered.missing,
            Self::Partial(partial) => &partial.missing,
        }
    }

    pub fn into_result(self) -> Result<Rendered, PartialRender> {
        match self {
            Self::Clean(rendered) => Ok(rendered),
            Self::Partial(partial) => Err(partial),
        }
    }
}

struct Failure {
    code: RenderErrorCode,
    message: String,
    fatal: bool,
    range: Option<ByteRange>,
}

impl Failure {
    fn recoverable(code: RenderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fatal: false,
            range: None,
        }
    }

    fn fatal(code: RenderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fatal: true,
            range: None,
        }
    }

    fn at(mut self, range: ByteRange) -> Self {
        if self.range.is_none() {
            self.range = Some(range);
        }
        self
    }
}

impl From<FilterFailure> for Failure {
    fn from(failure: FilterFailure) -> Self {
        match failure {
            FilterFailure::TooLarge => Self::recoverable(
                RenderErrorCode::ValueTooLarge,
                "intermediate value exceeds the size limit",
            ),
            FilterFailure::InvalidBase64 => {
                Self::recoverable(RenderErrorCode::InvalidBase64, "value is not valid base64")
            }
            FilterFailure::InvalidKey => {
                Self::recoverable(RenderErrorCode::InvalidArgument, "invalid HMAC key")
            }
        }
    }
}

type Eval<T> = Result<T, Failure>;

pub(crate) fn render(
    template: &Template,
    context: &Context,
    limits: &RenderLimits,
    env: &mut RenderEnv<'_>,
) -> RenderOutcome {
    let started = env.clock.elapsed();
    let mut run = Run {
        template,
        context,
        limits,
        env,
        started,
        evaluations: 0,
        lookups: 0,
        writer: BoundedWriter::new(
            template.target(),
            limits.max_output_bytes,
            template.source().len(),
        ),
        errors: Vec::new(),
        missing: Vec::new(),
        step_documents: BTreeMap::new(),
    };
    run.execute();
    run.finish()
}

struct Run<'a, 'r, 'e> {
    template: &'a Template,
    context: &'a Context,
    limits: &'a RenderLimits,
    env: &'r mut RenderEnv<'e>,
    started: Duration,
    evaluations: usize,
    lookups: usize,
    writer: BoundedWriter,
    errors: Vec<RenderError>,
    missing: Vec<MissingVariable>,
    step_documents: BTreeMap<String, Option<JsonValue>>,
}

impl<'a> Run<'a, '_, '_> {
    fn execute(&mut self) {
        for part in self.template.parts() {
            match part {
                Part::Text(range) => {
                    let text = self.template.slice(*range);
                    if let Err(failure) = self.writer.write_literal(text) {
                        let failure = match failure {
                            WriteFailure::LimitExceeded => self.output_limit(),
                            WriteFailure::ForbiddenCharacter => Failure::fatal(
                                RenderErrorCode::ForbiddenCharacter,
                                "literal text contains a forbidden character",
                            ),
                        };
                        self.push_error(failure.at(*range));
                        return;
                    }
                }
                Part::Expression(expression) => {
                    if let Err(failure) = self.run_expression(expression) {
                        let fatal = failure.fatal;
                        self.push_error(failure.at(expression.range));
                        if fatal || self.error_limit_hit() {
                            return;
                        }
                    }
                }
            }
        }
        self.check_status();
    }

    fn output_limit(&self) -> Failure {
        Failure::fatal(
            RenderErrorCode::OutputLimitExceeded,
            format!(
                "output exceeds the limit of {} bytes",
                self.limits.max_output_bytes
            ),
        )
    }

    fn error_limit_hit(&self) -> bool {
        self.errors
            .last()
            .is_some_and(|error| error.code == RenderErrorCode::ErrorLimitExceeded)
    }

    fn push_error(&mut self, failure: Failure) {
        if self.errors.len() >= self.limits.max_errors {
            if !self.error_limit_hit() {
                self.errors.push(RenderError {
                    code: RenderErrorCode::ErrorLimitExceeded,
                    message: "too many render errors".to_owned(),
                    position: locate(self.template.source(), ByteRange::new(0, 0)),
                });
            }
            return;
        }
        let range = failure.range.unwrap_or(ByteRange::new(0, 0));
        self.errors.push(RenderError {
            code: failure.code,
            message: failure.message,
            position: locate(self.template.source(), range),
        });
    }

    fn check_status(&mut self) {
        if self.template.target() != RenderTarget::Status || !self.errors.is_empty() {
            return;
        }
        let valid =
            self.writer.as_str().parse::<u16>().is_ok_and(|status| {
                self.writer.as_str().len() == 3 && (100..=599).contains(&status)
            });
        if !valid {
            self.push_error(Failure::fatal(
                RenderErrorCode::InvalidStatus,
                "rendered status is not a three digit code between 100 and 599",
            ));
        }
    }

    fn finish(self) -> RenderOutcome {
        let evaluations = self.evaluations;
        let missing = self.missing;
        let errors = self.errors;
        let output = self.writer.into_string();
        if errors.is_empty() {
            RenderOutcome::Clean(Rendered {
                output,
                missing,
                evaluations,
            })
        } else {
            RenderOutcome::Partial(PartialRender {
                output,
                errors,
                missing,
                evaluations,
            })
        }
    }

    fn tick(&mut self) -> Eval<()> {
        let elapsed = self.env.clock.elapsed().saturating_sub(self.started);
        if elapsed >= self.limits.deadline {
            return Err(Failure::fatal(
                RenderErrorCode::DeadlineExceeded,
                "render deadline exceeded",
            ));
        }
        if self.evaluations >= self.limits.max_evaluations {
            return Err(Failure::fatal(
                RenderErrorCode::EvaluationLimitExceeded,
                format!(
                    "more than {} evaluations were requested",
                    self.limits.max_evaluations
                ),
            ));
        }
        self.evaluations += 1;
        Ok(())
    }

    fn lookup_step(&mut self) -> Eval<()> {
        if self.lookups >= self.limits.max_json_lookups {
            return Err(Failure::fatal(
                RenderErrorCode::JsonLookupLimitExceeded,
                format!(
                    "more than {} lookups were requested",
                    self.limits.max_json_lookups
                ),
            ));
        }
        self.lookups += 1;
        Ok(())
    }

    fn run_expression(&mut self, expression: &'a Expression) -> Eval<()> {
        self.tick()?;
        let mut value = match &expression.primary {
            Primary::Reference(reference) => self.reference(reference)?,
            Primary::Function(function) => {
                self.tick()
                    .map_err(|failure| failure.at(expression.primary_range))?;
                self.function(function)
                    .map_err(|failure| failure.at(expression.primary_range))?
            }
            Primary::Text(text) => Value::borrowed(text),
            Primary::Int(number) => Value::Int(*number),
        };
        for filter in &expression.filters {
            self.tick().map_err(|failure| failure.at(filter.range))?;
            value = self
                .filter(&filter.kind, value)
                .map_err(|failure| failure.at(filter.range))?;
        }
        if matches!(value, Value::Missing) {
            self.note_missing(expression.primary_range);
        }
        let text = value.into_text().ok_or_else(|| {
            Failure::recoverable(RenderErrorCode::InvalidArgument, "time is out of range")
        })?;
        self.writer
            .write_value(&text)
            .map_err(|failure| match failure {
                WriteFailure::LimitExceeded => self.output_limit(),
                WriteFailure::ForbiddenCharacter => Failure::recoverable(
                    RenderErrorCode::ForbiddenCharacter,
                    "value contains a character that is not allowed in this output",
                ),
            })
    }

    fn note_missing(&mut self, range: ByteRange) {
        if self.missing.len() >= self.limits.max_errors {
            return;
        }
        let reference: String = self
            .template
            .slice(range)
            .chars()
            .take(MAX_NOTE_BYTES)
            .collect();
        self.missing.push(MissingVariable {
            reference,
            position: locate(self.template.source(), range),
        });
    }

    fn value_limit(&self) -> usize {
        self.limits.max_value_bytes
    }

    fn json_failure(_: crate::value::JsonConversion) -> Failure {
        Failure::recoverable(
            RenderErrorCode::ValueTooLarge,
            "JSON value exceeds the size limit",
        )
    }

    fn pairs_first<'p>(pairs: &'p [(String, String)], name: &str, fold: bool) -> Value<'p> {
        pairs
            .iter()
            .find(|(candidate, _)| {
                if fold {
                    candidate.eq_ignore_ascii_case(name)
                } else {
                    candidate == name
                }
            })
            .map_or(Value::Missing, |(_, value)| Value::borrowed(value))
    }

    fn navigate(
        &mut self,
        root: Option<&'a JsonValue>,
        tail: &[Segment],
    ) -> Eval<Option<&'a JsonValue>> {
        if tail.len() + 1 > self.limits.max_json_depth {
            return Err(Failure::recoverable(
                RenderErrorCode::JsonDepthExceeded,
                format!("path is deeper than {} levels", self.limits.max_json_depth),
            ));
        }
        self.lookup_step()?;
        let mut current = root;
        for segment in tail {
            self.lookup_step()?;
            current = match (current, segment) {
                (Some(JsonValue::Object(map)), Segment::Key(key)) => map.get(key.unpack()),
                (Some(JsonValue::Array(items)), Segment::Index(index)) => items.get(*index),
                _ => None,
            };
        }
        Ok(current)
    }

    fn named_json(
        &mut self,
        source: &'a BTreeMap<String, JsonValue>,
        name: &Key,
        tail: &[Segment],
    ) -> Eval<Value<'a>> {
        let found = self.navigate(source.get(name.unpack()), tail)?;
        match found {
            Some(value) => json_to_value(value, self.value_limit()).map_err(Self::json_failure),
            None => Ok(Value::Missing),
        }
    }

    fn headers_json(&self) -> Eval<Value<'a>> {
        let mut merged: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in &self.context.request.headers {
            let entry = merged.entry(name.to_ascii_lowercase()).or_default();
            if !entry.is_empty() {
                entry.push_str(", ");
            }
            entry.push_str(value);
        }
        compact_json(&merged, self.value_limit())
            .map(Value::owned)
            .map_err(Self::json_failure)
    }

    fn reference(&mut self, reference: &'a Reference) -> Eval<Value<'a>> {
        let context = self.context;
        let request = &context.request;
        Ok(match reference {
            Reference::RequestMethod => Value::borrowed(&request.method),
            Reference::RequestPath => Value::borrowed(&request.path),
            Reference::RequestSegment(index) => request
                .segments
                .get(*index)
                .map_or(Value::Missing, |segment| Value::borrowed(segment)),
            Reference::RequestQuery(key) => Self::pairs_first(&request.query, key.unpack(), false),
            Reference::RequestHeader(key) => {
                Self::pairs_first(&request.headers, key.unpack(), true)
            }
            Reference::RequestHeaders => self.headers_json()?,
            Reference::RequestForm(key) => Self::pairs_first(&request.form, key.unpack(), false),
            Reference::RequestBody => Value::borrowed(request.body.text()),
            Reference::RequestIp => Value::borrowed(&request.ip),
            Reference::RequestContentType => Value::borrowed(&request.content_type),
            Reference::RequestKind => Value::borrowed(&request.kind),
            Reference::RequestMeta(key) => Self::pairs_first(&request.meta, key.unpack(), false),
            Reference::RequestSignature => Value::borrowed(request.signature.as_str()),
            Reference::CaptureId => Value::borrowed(&context.capture.id),
            Reference::CaptureSeq => Value::Int(context.capture.seq),
            Reference::CaptureReceivedAt => Value::Time(context.capture.received_at),
            Reference::EndpointId => Value::borrowed(&context.endpoint.id),
            Reference::EndpointUrl => Value::borrowed(&context.endpoint.url),
            Reference::EndpointTitle => Value::borrowed(&context.endpoint.title),
            Reference::Var(name, tail) => self.named_json(&context.vars, name, tail)?,
            Reference::Store(name, tail) => self.named_json(&context.store, name, tail)?,
            Reference::Counter(name) => context
                .counters
                .get(name.unpack())
                .map_or(Value::Missing, |count| Value::Int(*count)),
            Reference::Step(name, field) => self.step_field(name, field),
            Reference::Secret(name) => context
                .secrets
                .unpack()
                .get(name.unpack())
                .map_or(Value::Missing, |secret| Value::borrowed(secret)),
            Reference::Now => Value::Time(context.now),
        })
    }

    fn step_field(&self, name: &Key, field: &'a StepField) -> Value<'a> {
        let Some(step) = self.context.steps.get(name.unpack()) else {
            return Value::Missing;
        };
        match field {
            StepField::Status => step
                .status
                .map_or(Value::Missing, |status| Value::Int(i64::from(status))),
            StepField::Header(key) => Self::pairs_first(&step.headers, key.unpack(), true),
            StepField::Body => Value::borrowed(&step.body),
            StepField::DurationMs => {
                Value::Int(i64::try_from(step.duration_ms).unwrap_or(i64::MAX))
            }
            StepField::Error => step
                .error
                .as_deref()
                .map_or(Value::Missing, Value::borrowed),
        }
    }

    fn function(&mut self, function: &'a Function) -> Eval<Value<'a>> {
        match function {
            Function::Json(source, path) => self.json_query(source, &path.path),
            Function::Uuid => {
                let mut bytes = [0u8; 16];
                self.env.random.fill_bytes(&mut bytes);
                Ok(Value::owned(
                    Builder::from_random_bytes(bytes).into_uuid().to_string(),
                ))
            }
            Function::RandomHex(length) => {
                let length = self.arg_int(length)?;
                if !(1..=MAX_RANDOM_HEX).contains(&length) {
                    return Err(Failure::recoverable(
                        RenderErrorCode::InvalidArgument,
                        "random_hex length must be between 1 and 64",
                    ));
                }
                let characters = usize::try_from(length).unwrap_or(0);
                let mut bytes = vec![0u8; characters.div_ceil(2)];
                self.env.random.fill_bytes(&mut bytes);
                let mut hex = filters::hex_encode(&bytes);
                hex.truncate(characters);
                Ok(Value::owned(hex))
            }
            Function::RandomInt(low, high) => {
                let low = self.arg_int(low)?;
                let high = self.arg_int(high)?;
                if low > high {
                    return Err(Failure::recoverable(
                        RenderErrorCode::InvalidArgument,
                        "random_int lower bound is above the upper bound",
                    ));
                }
                Ok(Value::Int(self.random_between(low, high)))
            }
        }
    }

    fn random_between(&mut self, low: i64, high: i64) -> i64 {
        let span = i128::from(high) - i128::from(low) + 1;
        let span = u128::try_from(span).unwrap_or(1);
        if span > u128::from(u64::MAX) {
            let mut bytes = [0u8; 8];
            self.env.random.fill_bytes(&mut bytes);
            return i64::from_le_bytes(bytes);
        }
        let span = u64::try_from(span).unwrap_or(1);
        let zone = u64::MAX - (u64::MAX % span);
        let mut draw;
        loop {
            let mut bytes = [0u8; 8];
            self.env.random.fill_bytes(&mut bytes);
            draw = u64::from_le_bytes(bytes);
            if draw < zone {
                break;
            }
        }
        let offset = i128::from(draw % span);
        i64::try_from(i128::from(low) + offset).unwrap_or(low)
    }

    fn json_query(
        &mut self,
        source: &'a JsonSource,
        path: &serde_json_path::JsonPath,
    ) -> Eval<Value<'a>> {
        self.lookup_step()?;
        let max = self.value_limit();
        match source {
            JsonSource::Request => {
                let found = self
                    .context
                    .request
                    .body
                    .json()
                    .and_then(|document| path.query(document).first());
                match found {
                    Some(value) => json_to_value(value, max).map_err(Self::json_failure),
                    None => Ok(Value::Missing),
                }
            }
            JsonSource::Step(name) => {
                if !self.step_documents.contains_key(name.unpack()) {
                    let document = self
                        .context
                        .steps
                        .get(name.unpack())
                        .and_then(|step| parse_json_document(step.body.as_bytes()));
                    self.step_documents
                        .insert(name.unpack().to_owned(), document);
                }
                let found = self
                    .step_documents
                    .get(name.unpack())
                    .and_then(Option::as_ref)
                    .and_then(|document| path.query(document).first());
                match found {
                    Some(value) => json_to_value(value, max)
                        .map(Value::into_owned)
                        .map_err(Self::json_failure),
                    None => Ok(Value::Missing),
                }
            }
        }
    }

    fn arg_value(&mut self, arg: &'a Arg) -> Eval<Value<'a>> {
        match arg {
            Arg::Reference(reference) => self.reference(reference),
            Arg::Text(text) => Ok(Value::borrowed(text)),
            Arg::Int(number) => Ok(Value::Int(*number)),
        }
    }

    fn arg_text(&mut self, arg: &'a Arg) -> Eval<Cow<'a, str>> {
        self.arg_value(arg)?.into_text().ok_or_else(|| {
            Failure::recoverable(RenderErrorCode::InvalidArgument, "time is out of range")
        })
    }

    fn arg_int(&mut self, arg: &'a Arg) -> Eval<i64> {
        match self.arg_value(arg)? {
            Value::Int(number) => Ok(number),
            Value::Text(text) => text.trim().parse::<i64>().map_err(|_| {
                Failure::recoverable(RenderErrorCode::TypeMismatch, "argument is not an integer")
            }),
            Value::Missing | Value::Time(_) => Err(Failure::recoverable(
                RenderErrorCode::TypeMismatch,
                "argument is not an integer",
            )),
        }
    }

    fn filter(&mut self, kind: &'a FilterKind, input: Value<'a>) -> Eval<Value<'a>> {
        let max = self.value_limit();
        match kind {
            FilterKind::Default(arg) => {
                if input.is_blank() {
                    self.arg_value(arg)
                } else {
                    Ok(input)
                }
            }
            FilterKind::Length => {
                let text = self.text_of(input)?;
                Ok(Value::Int(filters::length(&text)))
            }
            FilterKind::Date(arg) => {
                let instant = match input {
                    Value::Missing => return Ok(Value::Missing),
                    Value::Time(instant) => instant,
                    Value::Int(seconds) => Self::from_unix(seconds)?,
                    Value::Text(text) => {
                        let seconds = text.trim().parse::<i64>().map_err(|_| {
                            Failure::recoverable(
                                RenderErrorCode::TypeMismatch,
                                "value is not a timestamp",
                            )
                        })?;
                        Self::from_unix(seconds)?
                    }
                };
                let pattern = self.arg_text(arg)?;
                format::render(&pattern, instant, max)
                    .map(Value::owned)
                    .map_err(|problem| match problem {
                        FormatProblem::Invalid(message) => {
                            Failure::recoverable(RenderErrorCode::InvalidArgument, message)
                        }
                        FormatProblem::TooLarge => Failure::recoverable(
                            RenderErrorCode::ValueTooLarge,
                            "formatted date exceeds the size limit",
                        ),
                    })
            }
            _ => {
                let text = self.text_of(input)?;
                self.text_filter(kind, text, max)
            }
        }
    }

    fn from_unix(seconds: i64) -> Eval<OffsetDateTime> {
        OffsetDateTime::from_unix_timestamp(seconds).map_err(|_| {
            Failure::recoverable(
                RenderErrorCode::InvalidArgument,
                "timestamp is out of range",
            )
        })
    }

    fn text_of(&self, input: Value<'a>) -> Eval<Cow<'a, str>> {
        input.into_text().ok_or_else(|| {
            Failure::recoverable(RenderErrorCode::InvalidArgument, "time is out of range")
        })
    }

    fn text_filter(
        &mut self,
        kind: &'a FilterKind,
        text: Cow<'a, str>,
        max: usize,
    ) -> Eval<Value<'a>> {
        let produced = match kind {
            FilterKind::Upper => Value::owned(filters::upper(&text, max)?),
            FilterKind::Lower => Value::owned(filters::lower(&text, max)?),
            FilterKind::Trim => Value::Text(filters::trim(text)),
            FilterKind::Truncate(arg) => {
                let count = self.arg_int(arg)?;
                let count = usize::try_from(count).map_err(|_| {
                    Failure::recoverable(
                        RenderErrorCode::InvalidArgument,
                        "truncate length must not be negative",
                    )
                })?;
                Value::Text(filters::truncate(text, count))
            }
            FilterKind::Replace(from, to) => {
                let from = self.arg_text(from)?;
                let to = self.arg_text(to)?;
                if from.is_empty() {
                    return Err(Failure::recoverable(
                        RenderErrorCode::InvalidArgument,
                        "replace needs a non-empty search string",
                    ));
                }
                Value::owned(filters::replace(&text, &from, &to, max)?)
            }
            FilterKind::Json => Value::owned(filters::json_string(&text, max)?),
            FilterKind::Url => Value::owned(filters::url_encode(&text, max)?),
            FilterKind::Html => Value::owned(filters::html_escape(&text, max)?),
            FilterKind::Base64 => Value::owned(filters::base64_encode(&text, max)?),
            FilterKind::Base64Decode => Value::owned(filters::base64_decode(&text)?),
            FilterKind::Hex => Value::owned(filters::hex_text(&text, max)?),
            FilterKind::Sha256 => Value::owned(filters::sha256_hex(&text)),
            FilterKind::HmacSha256(key) => {
                let key = self.arg_text(key)?;
                Value::owned(filters::hmac_sha256_hex(&key, &text)?)
            }
            FilterKind::Default(_) | FilterKind::Length | FilterKind::Date(_) => Value::Text(text),
        };
        Ok(produced)
    }
}
