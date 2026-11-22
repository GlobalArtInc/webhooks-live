use utoipa::OpenApi;

use super::dto::{
    ScriptDiagnosticDto, ScriptFunctionDto, ScriptLimitsDto, ScriptMetaDto, ScriptParameterDto,
    ScriptPhaseLimitsDto, ScriptValidateInputDoc, ScriptValidationDto, ScriptVariableDto,
};
use super::handlers;

#[derive(OpenApi)]
#[openapi(
    paths(handlers::meta, handlers::validate),
    components(schemas(
        ScriptMetaDto,
        ScriptFunctionDto,
        ScriptParameterDto,
        ScriptVariableDto,
        ScriptLimitsDto,
        ScriptPhaseLimitsDto,
        ScriptValidateInputDoc,
        ScriptValidationDto,
        ScriptDiagnosticDto
    )),
    tags((name = "Script", description = "The script step language: its catalogue for the editor and a compile-only check of a draft source"))
)]
pub struct ScriptDoc;
