use serde_json::json;
use whl_template::{
    BodyData, CatalogueBasis, CatalogueEntry, CatalogueGroup, CatalogueType, DELIMITERS,
    MAX_SAMPLE_CHARS, NameValue, Policy, RenderLimits, RenderTarget, Template, catalogue,
};

use crate::common::{fixture, pairs, render_template};

fn entry<'a>(entries: &'a [CatalogueEntry], path: &str) -> &'a CatalogueEntry {
    entries
        .iter()
        .find(|entry| entry.path == path)
        .unwrap_or_else(|| panic!("no entry {path}"))
}

#[test]
fn every_entry_inserts_text_the_response_policy_parses() {
    let mut context = fixture();
    context.request.query = pairs(&[("mode", "test"), ("a-b", "1")]);
    context.request.headers = pairs(&[("Stripe-Signature", "t=1,v1=9a")]);
    context.request.body = BodyData::from_text(
        r#"{"id":"evt_1","data":{"amount":2000,"a-b":true,"1x":[1,2,3,4]},"list":[{"k":1}]}"#,
    );
    let policy = Policy::response(RenderTarget::Body);
    let entries = catalogue(&context, CatalogueBasis::Capture);
    assert!(entries.len() > 30);
    for entry in &entries {
        assert!(
            entry.insert_text.starts_with(DELIMITERS.open)
                && entry.insert_text.ends_with(DELIMITERS.close),
            "{}",
            entry.path
        );
        Template::parse(&entry.insert_text, &policy)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", entry.path));
    }
}

#[test]
fn samples_are_what_the_engine_renders() {
    let entries = catalogue(&fixture(), CatalogueBasis::Capture);
    assert_eq!(
        entry(&entries, "request.method").sample.as_deref(),
        Some("\"POST\"")
    );
    assert_eq!(entry(&entries, "capture.seq").sample.as_deref(), Some("7"));
    assert_eq!(
        entry(&entries, "now").sample.as_deref(),
        Some("2024-03-05T14:07:09Z")
    );
    assert_eq!(
        entry(&entries, "request.query.name").sample.as_deref(),
        Some("\"Zoë & co\"")
    );
    assert_eq!(
        entry(&entries, "request.header.x-token").sample.as_deref(),
        Some("\"abc\"")
    );
}

#[test]
fn values_the_request_lacks_have_no_sample() {
    let mut context = fixture();
    context.endpoint.title = String::new();
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let title = entry(&entries, "endpoint.title");
    assert_eq!(title.sample.as_deref(), Some("\"\""));
    assert_eq!(title.value_type, CatalogueType::String);
    let mut bare = fixture();
    bare.request.query.clear();
    let entries = catalogue(&bare, CatalogueBasis::Capture);
    assert!(
        entries
            .iter()
            .all(|entry| !entry.path.starts_with("request.query."))
    );
}

#[test]
fn request_names_become_paths_with_lowercase_headers_and_no_duplicates() {
    let mut context = fixture();
    context.request.query = pairs(&[("a", "1"), ("a", "2"), ("bad name", "x")]);
    context.request.headers = pairs(&[("X-Token", "abc"), ("x-token", "def")]);
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let queries: Vec<&str> = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .filter(|path| path.starts_with("request.query."))
        .collect();
    assert_eq!(queries, ["request.query.a"]);
    let headers: Vec<&str> = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .filter(|path| path.starts_with("request.header."))
        .collect();
    assert_eq!(headers, ["request.header.x-token"]);
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "request.segments[1]")
    );
}

#[test]
fn json_body_keys_become_json_calls_with_types() {
    let mut context = fixture();
    context.request.body = BodyData::from_text(
        r#"{"id":"evt_1","data":{"amount":2000,"a-b":true,"1x":[1,2,3,4]},"none":null,"ratio":1.5}"#,
    );
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let id = entry(&entries, "json(\"$.id\")");
    assert_eq!(id.insert_text, "{{ json(\"$.id\") }}");
    assert_eq!(id.value_type, CatalogueType::String);
    assert_eq!(id.sample.as_deref(), Some("\"evt_1\""));
    assert_eq!(
        entry(&entries, "json(\"$.data.amount\")").value_type,
        CatalogueType::Integer
    );
    assert_eq!(
        entry(&entries, "json(\"$.data['a-b']\")").value_type,
        CatalogueType::Boolean
    );
    assert_eq!(
        entry(&entries, "json(\"$.data['1x'][2]\")")
            .sample
            .as_deref(),
        Some("3")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.path != "json(\"$.data['1x'][3]\")")
    );
    assert_eq!(
        entry(&entries, "json(\"$.none\")").value_type,
        CatalogueType::Null
    );
    assert_eq!(
        entry(&entries, "json(\"$.ratio\")").value_type,
        CatalogueType::Number
    );
    assert_eq!(
        entry(&entries, "json(\"$.data\")").value_type,
        CatalogueType::Object
    );
}

#[test]
fn json_entries_are_bounded_in_count_and_depth() {
    let mut context = fixture();
    let wide: serde_json::Map<String, serde_json::Value> = (0..200)
        .map(|index| (format!("k{index}"), json!(index)))
        .collect();
    context.request.body = BodyData::from_text(&serde_json::Value::Object(wide).to_string());
    let calls = catalogue(&context, CatalogueBasis::Capture)
        .iter()
        .filter(|entry| entry.group == CatalogueGroup::Request && entry.path.starts_with("json("))
        .count();
    assert_eq!(calls, 50);
    context.request.body = BodyData::from_text(r#"{"a":{"b":{"c":{"d":{"e":1}}}}}"#);
    let entries = catalogue(&context, CatalogueBasis::Capture);
    assert!(
        entries
            .iter()
            .any(|entry| entry.path == "json(\"$.a.b.c.d\")")
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry.path != "json(\"$.a.b.c.d.e\")")
    );
}

#[test]
fn long_samples_are_shortened() {
    let mut context = fixture();
    context.request.body = BodyData::from_text(&json!({"text": "x".repeat(500)}).to_string());
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let sample = entry(&entries, "json(\"$.text\")")
        .sample
        .as_deref()
        .unwrap_or("");
    assert_eq!(sample.chars().count(), MAX_SAMPLE_CHARS);
    assert!(sample.ends_with('\u{2026}'));
}

#[test]
fn persistent_variables_are_listed_and_run_variables_steps_and_secrets_are_not() {
    let entries = catalogue(&fixture(), CatalogueBasis::Capture);
    let stored = entry(&entries, "store.s");
    assert_eq!(stored.group, CatalogueGroup::Variables);
    assert_eq!(stored.sample.as_deref(), Some("\"stored\""));
    let counter = entry(&entries, "counter.hits");
    assert_eq!(counter.value_type, CatalogueType::Integer);
    assert_eq!(counter.sample.as_deref(), Some("3"));
    for entry in &entries {
        assert!(
            !entry.path.starts_with("var.")
                && !entry.path.starts_with("step.")
                && !entry.path.starts_with("secret."),
            "{}",
            entry.path
        );
    }
}

#[test]
fn groups_keep_a_stable_order_and_helpers_have_no_sample() {
    let entries = catalogue(&fixture(), CatalogueBasis::Capture);
    let mut order: Vec<CatalogueGroup> = Vec::new();
    for entry in &entries {
        if order.last() != Some(&entry.group) {
            order.push(entry.group);
        }
    }
    assert_eq!(
        order,
        [
            CatalogueGroup::Request,
            CatalogueGroup::Time,
            CatalogueGroup::Variables,
            CatalogueGroup::Helpers
        ]
    );
    let helpers: Vec<&CatalogueEntry> = entries
        .iter()
        .filter(|entry| entry.group == CatalogueGroup::Helpers)
        .collect();
    assert!(helpers.iter().any(|entry| entry.path == "uuid()"));
    assert!(helpers.iter().all(|entry| entry.sample.is_none()
        && entry.value_type == CatalogueType::Function
        && entry.description.is_some()));
    assert_eq!(catalogue(&fixture(), CatalogueBasis::Capture), entries);
}

#[test]
fn without_a_capture_only_the_fixed_names_the_variables_and_the_helpers_remain() {
    let entries = catalogue(&fixture(), CatalogueBasis::NoCapture);
    assert!(entries.iter().any(|entry| entry.path == "request.method"));
    assert!(entries.iter().any(|entry| entry.path == "counter.hits"));
    assert!(entries.iter().any(|entry| entry.path == "uuid()"));
    assert!(entries.iter().all(|entry| entry.path != "json(\"$.order\")"
        && !entry.path.starts_with("request.query.")
        && !entry.path.starts_with("request.header.")));
    let fixed: Vec<&CatalogueEntry> = entries
        .iter()
        .filter(|entry| entry.group != CatalogueGroup::Variables)
        .collect();
    assert!(fixed.iter().all(|entry| entry.sample.is_none()));
    assert_eq!(entry(&entries, "counter.hits").sample.as_deref(), Some("3"));
}

#[test]
fn json_keys_keep_their_own_budget_beside_many_headers_and_query_fields() {
    let mut context = fixture();
    let names: Vec<String> = (0..40).map(|index| format!("x-h{index}")).collect();
    let queries: Vec<String> = (0..40).map(|index| format!("q{index}")).collect();
    context.request.headers = names.iter().map(|name| NameValue::new(name, "v")).collect();
    context.request.query = queries
        .iter()
        .map(|name| NameValue::new(name, "v"))
        .collect();
    let keys: serde_json::Map<String, serde_json::Value> = (0..50)
        .map(|index| (format!("k{index}"), json!({"deep": index})))
        .collect();
    context.request.body = BodyData::from_text(&serde_json::Value::Object(keys).to_string());
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let top: Vec<&CatalogueEntry> = entries
        .iter()
        .filter(|entry| entry.path.starts_with("json(\"$.k") && !entry.path.contains(".deep"))
        .collect();
    assert_eq!(top.len(), 50);
    let json_paths: Vec<&str> = entries
        .iter()
        .map(|entry| entry.path.as_str())
        .filter(|path| path.starts_with("json(\"$.k"))
        .collect();
    assert_eq!(json_paths.len(), 100);
    assert!(json_paths[..50].iter().all(|path| !path.contains(".deep")));
}

#[test]
fn hostile_json_keys_never_produce_an_expression_that_changes_meaning() {
    let mut context = fixture();
    context.request.body = BodyData::from_text(
        r#"{"a\"b":1,"c\\d":2,"e)f":3,"g}}h":4,"i'j":5,"ünï":6,"k\nl":7,"m n":8,"ok":9,"a-b":10}"#,
    );
    let entries = catalogue(&context, CatalogueBasis::Capture);
    let policy = Policy::response(RenderTarget::Body);
    let generated: Vec<&CatalogueEntry> = entries
        .iter()
        .filter(|entry| {
            entry.group == CatalogueGroup::Request && entry.path.starts_with("json(\"$")
        })
        .collect();
    assert!(generated.iter().any(|entry| entry.path == "json(\"$.ok\")"));
    assert!(
        generated
            .iter()
            .any(|entry| entry.path == "json(\"$['a-b']\")")
    );
    for entry in generated {
        let template = Template::parse(&entry.insert_text, &policy)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", entry.path));
        let outcome = render_template(
            &template,
            &context,
            &RenderLimits::for_target(RenderTarget::Body),
        );
        assert!(outcome.is_clean(), "{}", entry.path);
        assert_eq!(
            Some(outcome.output()),
            entry.sample.as_deref(),
            "{}",
            entry.path
        );
    }
}
