use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{document, registry};

const OPERATIONS: [&str; 8] = [
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

fn json() -> Value {
    serde_json::to_value(document()).expect("the document serialises")
}

fn operations(document: &Value) -> Vec<(String, String, &Value)> {
    let paths = document["paths"]
        .as_object()
        .expect("the document has paths");
    paths
        .iter()
        .flat_map(|(path, item)| {
            OPERATIONS.iter().filter_map(move |method| {
                item.get(*method)
                    .map(|operation| ((*method).to_owned(), path.clone(), operation))
            })
        })
        .collect()
}

fn references<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                match (key.as_str(), inner) {
                    ("$ref", Value::String(target)) => found.push(target),
                    _ => references(inner, found),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| references(item, found)),
        _ => {}
    }
}

fn template_parameters(path: &str) -> BTreeSet<String> {
    path.split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}'))
        .map(|(name, _)| name.to_owned())
        .collect()
}

#[test]
fn the_document_is_openapi_3_1_with_the_build_version_and_a_relative_server() {
    let document = json();
    assert!(
        document["openapi"]
            .as_str()
            .is_some_and(|version| version.starts_with("3.1")),
        "{}",
        document["openapi"]
    );
    assert_eq!(document["info"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(document["servers"][0]["url"], "/api/v1");
    assert!(document["components"]["securitySchemes"]["session_cookie"].is_object());
    assert_eq!(
        document["components"]["securitySchemes"]["api_token"]["scheme"],
        "bearer"
    );
}

#[test]
fn every_reference_resolves() {
    let document = json();
    let mut found = Vec::new();
    references(&document, &mut found);
    assert!(!found.is_empty());
    let dangling: Vec<&str> = found
        .into_iter()
        .filter(|target| {
            target
                .strip_prefix('#')
                .is_none_or(|pointer| document.pointer(pointer).is_none())
        })
        .collect();
    assert!(dangling.is_empty(), "unresolved references: {dangling:?}");
}

#[test]
fn every_operation_has_a_response_an_unique_operation_id_and_a_tag() {
    let document = json();
    let mut ids = BTreeSet::new();
    for (method, path, operation) in operations(&document) {
        let name = format!("{method} {path}");
        assert!(
            operation["responses"]
                .as_object()
                .is_some_and(|responses| !responses.is_empty()),
            "{name} has no response"
        );
        let id = operation["operationId"]
            .as_str()
            .unwrap_or_else(|| panic!("{name} has no operationId"));
        assert!(ids.insert(id.to_owned()), "{name} repeats operationId {id}");
        assert!(
            operation["tags"]
                .as_array()
                .is_some_and(|tags| !tags.is_empty()),
            "{name} has no tag"
        );
    }
}

#[test]
fn every_path_template_parameter_is_declared_as_a_path_parameter() {
    let document = json();
    for (method, path, operation) in operations(&document) {
        let declared: BTreeSet<String> = operation["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|parameter| parameter["in"] == "path")
            .filter_map(|parameter| parameter["name"].as_str().map(str::to_owned))
            .collect();
        assert_eq!(
            declared,
            template_parameters(&path),
            "{method} {path} declares other path parameters than its template"
        );
    }
}

#[test]
fn every_security_requirement_names_a_defined_scheme() {
    let document = json();
    let schemes = document["components"]["securitySchemes"]
        .as_object()
        .expect("security schemes are defined");
    for (method, path, operation) in operations(&document) {
        for requirement in operation["security"].as_array().into_iter().flatten() {
            for name in requirement
                .as_object()
                .into_iter()
                .flat_map(|map| map.keys())
            {
                assert!(
                    schemes.contains_key(name),
                    "{method} {path} requires the undefined scheme {name}"
                );
            }
        }
    }
}

#[test]
fn area_documents_never_claim_the_same_operation_or_a_different_schema_of_the_same_name() {
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    let mut schemas: BTreeMap<String, Value> = BTreeMap::new();
    for area in registry::area_docs() {
        let area = serde_json::to_value(area).expect("the area document serialises");
        for (method, path, _) in operations(&area) {
            assert!(
                claimed.insert(format!("{method} {path}")),
                "two area documents claim {method} {path}"
            );
        }
        let area_schemas = area["components"]["schemas"].as_object();
        for (name, schema) in area_schemas.into_iter().flatten() {
            let previous = schemas.insert(name.clone(), schema.clone());
            assert!(
                previous.is_none_or(|previous| previous == *schema),
                "two area documents define the schema {name} differently"
            );
        }
    }
}

#[test]
fn the_token_input_schema_keeps_the_limits_the_parser_enforces() {
    let document = json();
    let schema = &document["components"]["schemas"]["ApiTokenInput"]["properties"];
    assert_eq!(
        schema["name"]["maxLength"],
        u64::try_from(whl_store::MAX_TOKEN_NAME_CHARS).expect("fits")
    );
    assert_eq!(
        schema["endpoint_ids"]["maxItems"],
        u64::try_from(whl_store::MAX_TOKEN_ENDPOINTS).expect("fits")
    );
    assert_eq!(
        schema["scopes"]["maxItems"],
        u64::try_from(whl_store::TokenScope::ALL.len()).expect("fits")
    );
}
