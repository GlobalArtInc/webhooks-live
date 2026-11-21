use std::sync::OnceLock;

use rhai::packages::{
    BasicArrayPackage, BasicMapPackage, BasicMathPackage, CorePackage, LogicPackage,
    MoreStringPackage, Package,
};
use rhai::{Engine, Map, Module, Scope, Shared};

use crate::limits::EngineLimits;

pub(crate) const REQUEST_VARIABLE: &str = "request";
pub(crate) const VARS_VARIABLE: &str = "vars";

pub(crate) const DISABLED_SYMBOLS: [&str; 9] = [
    "eval",
    "import",
    "export",
    "Fn",
    "call",
    "curry",
    "is_def_fn",
    "is_def_var",
    "global",
];

struct Packages {
    modules: Vec<Shared<Module>>,
}

static PACKAGES: OnceLock<Packages> = OnceLock::new();

fn packages() -> &'static Packages {
    PACKAGES.get_or_init(|| Packages {
        modules: vec![
            CorePackage::new().as_shared_module(),
            LogicPackage::new().as_shared_module(),
            BasicArrayPackage::new().as_shared_module(),
            BasicMapPackage::new().as_shared_module(),
            BasicMathPackage::new().as_shared_module(),
            MoreStringPackage::new().as_shared_module(),
        ],
    })
}

pub(crate) fn warm_up() {
    let _ = packages();
}

pub(crate) fn build_engine(limits: &EngineLimits) -> Engine {
    let mut engine = Engine::new_raw();
    for module in &packages().modules {
        engine.register_global_module(Shared::clone(module));
    }
    engine.set_max_call_levels(limits.max_call_levels);
    engine.set_max_expr_depths(
        limits.max_expression_depth,
        limits.max_function_expression_depth,
    );
    engine.set_max_string_size(limits.max_string_bytes);
    engine.set_max_array_size(limits.max_array_size);
    engine.set_max_map_size(limits.max_map_size);
    engine.set_max_functions(limits.max_functions);
    engine.set_max_variables(limits.max_variables);
    engine.set_strict_variables(true);
    for symbol in DISABLED_SYMBOLS {
        engine.disable_symbol(symbol);
    }
    engine
}

pub(crate) fn placeholder_scope() -> Scope<'static> {
    let mut scope = Scope::new();
    scope.push_constant(REQUEST_VARIABLE, Map::new());
    scope.push(VARS_VARIABLE, Map::new());
    scope
}
