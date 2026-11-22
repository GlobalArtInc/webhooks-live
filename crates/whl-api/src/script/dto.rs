use serde::Serialize;
use utoipa::ToSchema;
use whl_script::{
    CompileError, EngineLimits, FUNCTIONS, FunctionDoc, KEYWORDS, LANGUAGE_NAME, ParameterDoc,
    VARIABLES, VariableDoc,
};

const REFERENCE_URL: &str = "/docs/api/script_meta";

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptParameter)]
pub struct ScriptParameterDto {
    #[schema(example = "text")]
    name: &'static str,
    #[serde(rename = "type")]
    #[schema(example = "string")]
    kind: &'static str,
}

impl From<&ParameterDoc> for ScriptParameterDto {
    fn from(parameter: &ParameterDoc) -> Self {
        Self {
            name: parameter.name,
            kind: parameter.kind,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptFunction)]
pub struct ScriptFunctionDto {
    #[schema(example = "sha256")]
    name: &'static str,
    #[schema(example = "sha256(text: string) -> string")]
    signature: String,
    doc: &'static str,
    parameters: Vec<ScriptParameterDto>,
    #[schema(example = "string")]
    returns: Option<&'static str>,
}

impl From<&FunctionDoc> for ScriptFunctionDto {
    fn from(function: &FunctionDoc) -> Self {
        Self {
            name: function.name,
            signature: function.signature(),
            doc: function.doc,
            parameters: function
                .parameters
                .iter()
                .map(ScriptParameterDto::from)
                .collect(),
            returns: function.returns,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptVariable)]
pub struct ScriptVariableDto {
    #[schema(example = "request.method")]
    name: &'static str,
    #[serde(rename = "type")]
    #[schema(example = "string")]
    kind: &'static str,
    doc: &'static str,
}

impl From<&VariableDoc> for ScriptVariableDto {
    fn from(variable: &VariableDoc) -> Self {
        Self {
            name: variable.name,
            kind: variable.kind,
            doc: variable.doc,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptPhaseLimits)]
pub struct ScriptPhaseLimitsDto {
    #[schema(minimum = 1)]
    max_operations: u64,
    #[schema(minimum = 1)]
    max_time_ms: u64,
}

impl ScriptPhaseLimitsDto {
    pub(super) fn new(max_operations: u64, max_time_ms: u64) -> Self {
        Self {
            max_operations,
            max_time_ms,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptLimits)]
pub struct ScriptLimitsDto {
    #[schema(minimum = 1)]
    max_source_bytes: u64,
    #[schema(minimum = 1)]
    memory_bytes: u64,
    before_response: ScriptPhaseLimitsDto,
    after_response: ScriptPhaseLimitsDto,
}

impl ScriptLimitsDto {
    pub(super) fn new(
        engine: &EngineLimits,
        memory_bytes: usize,
        before: ScriptPhaseLimitsDto,
        after: ScriptPhaseLimitsDto,
    ) -> Self {
        Self {
            max_source_bytes: widen(engine.max_source_bytes),
            memory_bytes: widen(memory_bytes),
            before_response: before,
            after_response: after,
        }
    }
}

fn widen(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptMeta)]
pub struct ScriptMetaDto {
    #[schema(example = "rhai")]
    name: &'static str,
    keywords: Vec<&'static str>,
    functions: Vec<ScriptFunctionDto>,
    variables: Vec<ScriptVariableDto>,
    limits: ScriptLimitsDto,
    #[schema(example = "/docs/api/script_meta")]
    reference_url: &'static str,
}

impl ScriptMetaDto {
    pub(super) fn new(limits: ScriptLimitsDto) -> Self {
        Self {
            name: LANGUAGE_NAME,
            keywords: KEYWORDS.to_vec(),
            functions: FUNCTIONS.iter().map(ScriptFunctionDto::from).collect(),
            variables: VARIABLES.iter().map(ScriptVariableDto::from).collect(),
            limits,
            reference_url: REFERENCE_URL,
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptDiagnostic)]
pub struct ScriptDiagnosticDto {
    #[schema(pattern = "^(error|warning)$", example = "error")]
    severity: &'static str,
    #[schema(example = "unexpected_end")]
    code: &'static str,
    message: String,
    #[schema(minimum = 1)]
    line: u32,
    #[schema(minimum = 1)]
    column: u32,
    #[schema(minimum = 1)]
    length: u32,
}

#[derive(Serialize, ToSchema)]
#[schema(as = ScriptValidation)]
pub struct ScriptValidationDto {
    diagnostics: Vec<ScriptDiagnosticDto>,
}

impl ScriptValidationDto {
    pub(super) fn clean() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn from_error(error: &CompileError) -> Self {
        Self {
            diagnostics: error
                .diagnostics
                .iter()
                .map(|diagnostic| ScriptDiagnosticDto {
                    severity: "error",
                    code: diagnostic.code.as_str(),
                    message: diagnostic.message.clone(),
                    line: diagnostic.position.line,
                    column: diagnostic.position.column,
                    length: u32::try_from(diagnostic.position.length)
                        .unwrap_or(u32::MAX)
                        .max(1),
                })
                .collect(),
        }
    }
}

#[derive(ToSchema)]
#[schema(as = ScriptValidateInput)]
#[allow(dead_code)]
pub struct ScriptValidateInputDoc {
    #[schema(max_length = 16384)]
    source: String,
    #[schema(pattern = "^(before|after)$", example = "before")]
    phase: Option<String>,
}
