use utoipa::OpenApi;

use super::dry_run;
use super::dto::{RuleDryRunInputDoc, RuleDto, RuleInputDoc, RuleOrderInputDoc, RuleSetDto};
use super::handlers;
use super::runs::{self, RuleRunDto, RunListDto};

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::list,
        handlers::show,
        handlers::create,
        handlers::update,
        handlers::delete,
        handlers::reorder,
        dry_run::dry_run,
        runs::list
    ),
    components(schemas(
        RuleDto,
        RuleSetDto,
        RuleInputDoc,
        RuleDryRunInputDoc,
        RuleOrderInputDoc,
        RuleRunDto,
        RunListDto
    )),
    tags((name = "Rules", description = "Endpoint rules: conditions and steps, dry runs, reordering and the run history"))
)]
pub struct RulesDoc;
