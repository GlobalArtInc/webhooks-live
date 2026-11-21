use std::sync::Arc;

use rhai::{AST, LexError, ParseError, ParseErrorType};
use sha2::{Digest, Sha256};

use crate::engine::{DISABLED_SYMBOLS, build_engine, placeholder_scope, warm_up};
use crate::error::{CompileError, Diagnostic, ErrorCode, Position, bounded_message};
use crate::limits::{EngineLimits, LimitsError};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptHash([u8; 32]);

impl ScriptHash {
    pub fn of(source: &str) -> Self {
        Self(Sha256::digest(source.as_bytes()).into())
    }

    pub fn unpack(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl std::fmt::Debug for ScriptHash {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "ScriptHash({})", self.to_hex())
    }
}

#[derive(Clone)]
pub struct CompiledScript {
    ast: Arc<AST>,
    hash: ScriptHash,
    limits: EngineLimits,
    source_bytes: usize,
}

impl CompiledScript {
    pub fn hash(&self) -> ScriptHash {
        self.hash
    }

    pub fn limits(&self) -> &EngineLimits {
        &self.limits
    }

    pub fn source_bytes(&self) -> usize {
        self.source_bytes
    }

    pub(crate) fn ast(&self) -> &AST {
        &self.ast
    }
}

impl std::fmt::Debug for CompiledScript {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompiledScript")
            .field("hash", &self.hash)
            .field("source_bytes", &self.source_bytes)
            .finish()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScriptCompiler {
    limits: EngineLimits,
}

impl ScriptCompiler {
    pub fn new(limits: EngineLimits) -> Result<Self, LimitsError> {
        limits.validate()?;
        warm_up();
        Ok(Self { limits })
    }

    pub fn limits(&self) -> &EngineLimits {
        &self.limits
    }

    pub fn compile(&self, source: &str) -> Result<CompiledScript, CompileError> {
        if source.len() > self.limits.max_source_bytes {
            return Err(single(Diagnostic {
                code: ErrorCode::SourceTooLarge,
                message: format!(
                    "source is {} bytes, the limit is {}",
                    source.len(),
                    self.limits.max_source_bytes
                ),
                position: locate(source, 0),
            }));
        }
        let engine = build_engine(&self.limits);
        let scope = placeholder_scope();
        let ast = engine
            .compile_with_scope(&scope, source)
            .map_err(|error| single(diagnostic(&error, source)))?;
        Ok(CompiledScript {
            ast: Arc::new(ast),
            hash: ScriptHash::of(source),
            limits: self.limits,
            source_bytes: source.len(),
        })
    }
}

fn single(diagnostic: Diagnostic) -> CompileError {
    CompileError {
        diagnostics: vec![diagnostic],
    }
}

fn offset_of(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    let mut remaining = line.saturating_sub(1);
    for text in source.split_inclusive('\n') {
        if remaining == 0 {
            break;
        }
        offset += text.len();
        remaining -= 1;
    }
    if remaining > 0 {
        return source.len();
    }
    let rest = source.get(offset..).unwrap_or("");
    let skip = column.saturating_sub(1);
    let within = rest
        .char_indices()
        .nth(skip)
        .map_or(rest.trim_end_matches(['\n', '\r']).len(), |(index, _)| {
            index
        });
    offset + within
}

pub(crate) fn locate(source: &str, offset: usize) -> Position {
    let start = offset.min(source.len());
    let before = source.get(..start).unwrap_or("");
    let newlines = before.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let column = before
        .get(line_start..)
        .map_or(0, |line| line.chars().count());
    let length = source
        .get(start..)
        .and_then(|rest| rest.chars().next())
        .filter(|character| *character != '\n' && *character != '\r')
        .map_or(0, |_| 1);
    Position {
        offset: start,
        line: u32::try_from(newlines + 1).unwrap_or(u32::MAX),
        column: u32::try_from(column + 1).unwrap_or(u32::MAX),
        length,
    }
}

fn code_of(kind: &ParseErrorType) -> ErrorCode {
    match kind {
        ParseErrorType::UnexpectedEOF => ErrorCode::UnexpectedEnd,
        ParseErrorType::Reserved(_) | ParseErrorType::UnknownOperator(_) => {
            ErrorCode::DisabledFeature
        }
        ParseErrorType::ExprTooDeep => ErrorCode::ExpressionTooDeep,
        ParseErrorType::TooManyFunctions => ErrorCode::TooManyFunctions,
        ParseErrorType::LiteralTooLarge(..) => ErrorCode::LiteralTooLarge,
        ParseErrorType::VariableUndefined(_) => ErrorCode::UndefinedVariable,
        ParseErrorType::AssignmentToConstant(_) => ErrorCode::AssignmentToConstant,
        ParseErrorType::WrongFnDefinition
        | ParseErrorType::FnDuplicatedDefinition(..)
        | ParseErrorType::FnMissingName
        | ParseErrorType::FnMissingParams(_)
        | ParseErrorType::FnDuplicatedParam(..)
        | ParseErrorType::FnMissingBody(_) => ErrorCode::InvalidFunction,
        ParseErrorType::BadInput(lex) => match lex {
            LexError::ImproperSymbol(symbol, _) if DISABLED_SYMBOLS.contains(&symbol.as_str()) => {
                ErrorCode::DisabledFeature
            }
            LexError::UnexpectedInput(_) | LexError::ImproperSymbol(..) => ErrorCode::Syntax,
            LexError::UnterminatedString => ErrorCode::UnexpectedEnd,
            LexError::StringTooLong(_) => ErrorCode::LiteralTooLarge,
            LexError::MalformedEscapeSequence(_)
            | LexError::MalformedNumber(_)
            | LexError::MalformedChar(_) => ErrorCode::InvalidLiteral,
            _ => ErrorCode::Syntax,
        },
        _ => ErrorCode::Syntax,
    }
}

fn diagnostic(error: &ParseError, source: &str) -> Diagnostic {
    let line = error.1.line();
    let column = error.1.position();
    let offset = match (line, column) {
        (Some(line), Some(column)) => offset_of(source, line, column),
        (Some(line), None) => offset_of(source, line, 1),
        _ => source.len(),
    };
    Diagnostic {
        code: code_of(&error.0),
        message: bounded_message(&error.0.to_string()),
        position: locate(source, offset),
    }
}
