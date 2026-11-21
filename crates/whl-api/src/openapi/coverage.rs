use std::collections::BTreeSet;

use serde_json::Value;

use super::undocumented;
use crate::api_routes;

const OPERATIONS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RouteKey {
    method: String,
    path: String,
}

impl RouteKey {
    fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_ascii_uppercase(),
            path: normalise(path),
        }
    }
}

fn normalise(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut inside = false;
    for character in path.chars() {
        match character {
            '{' => {
                inside = true;
                out.push('{');
            }
            '}' => {
                inside = false;
                out.push('}');
            }
            _ if inside => {}
            _ => out.push(character),
        }
    }
    out
}

fn routed() -> BTreeSet<RouteKey> {
    api_routes()
        .entries()
        .iter()
        .map(|entry| RouteKey::new(entry.method.as_str(), entry.path))
        .collect()
}

fn documented() -> BTreeSet<RouteKey> {
    let document = serde_json::to_value(super::document()).expect("the document serialises");
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");
    paths
        .iter()
        .flat_map(|(path, item)| {
            OPERATIONS
                .iter()
                .filter(|method| item.get(**method).is_some_and(Value::is_object))
                .map(|method| RouteKey::new(method, path))
        })
        .collect()
}

fn allowlisted() -> BTreeSet<RouteKey> {
    undocumented::all()
        .into_iter()
        .map(|route| RouteKey::new(route.method, route.path))
        .collect()
}

fn render(keys: &BTreeSet<RouteKey>) -> Vec<String> {
    keys.iter()
        .map(|key| format!("{} {}", key.method, key.path))
        .collect()
}

#[test]
fn a_route_that_is_neither_documented_nor_allowlisted_fails() {
    let covered: BTreeSet<RouteKey> = documented().union(&allowlisted()).cloned().collect();
    let missing: BTreeSet<RouteKey> = routed().difference(&covered).cloned().collect();
    assert!(
        missing.is_empty(),
        "undocumented routes, annotate them with #[utoipa::path] and register the area doc: {:#?}",
        render(&missing)
    );
}

#[test]
fn an_allowlisted_route_that_is_documented_must_leave_the_allowlist() {
    let stale: BTreeSet<RouteKey> = allowlisted().intersection(&documented()).cloned().collect();
    assert!(
        stale.is_empty(),
        "documented routes still allowlisted, remove them from undocumented.rs: {:#?}",
        render(&stale)
    );
}

#[test]
fn an_allowlisted_route_that_is_not_routed_must_leave_the_allowlist() {
    let unknown: BTreeSet<RouteKey> = allowlisted().difference(&routed()).cloned().collect();
    assert!(
        unknown.is_empty(),
        "allowlisted routes the router does not serve: {:#?}",
        render(&unknown)
    );
}

#[test]
fn a_documented_path_that_is_not_routed_fails() {
    let phantom: BTreeSet<RouteKey> = documented().difference(&routed()).cloned().collect();
    assert!(
        phantom.is_empty(),
        "documented operations without a route: {:#?}",
        render(&phantom)
    );
}

#[test]
fn path_templates_compare_without_their_parameter_names() {
    assert_eq!(
        RouteKey::new("get", "/endpoints/{id}/captures/{capture_id}"),
        RouteKey::new("GET", "/endpoints/{endpoint}/captures/{c}")
    );
    assert_ne!(
        RouteKey::new("GET", "/endpoints/{id}"),
        RouteKey::new("DELETE", "/endpoints/{id}")
    );
}
