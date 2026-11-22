use utoipa::openapi::path::{Operation, PathItem};
use utoipa::openapi::{OpenApi, Ref, RefOr};

const COMMON: [(&str, &str); 2] = [("429", "RateLimited"), ("500", "InternalError")];

pub(super) fn add_common_responses(document: &mut OpenApi) {
    for item in document.paths.paths.values_mut() {
        for (_, operation) in operations(item) {
            for (status, name) in COMMON {
                operation
                    .responses
                    .responses
                    .entry(status.to_owned())
                    .or_insert_with(|| RefOr::Ref(Ref::from_response_name(name)));
            }
        }
    }
}

pub(super) fn operations(item: &mut PathItem) -> Vec<(&'static str, &mut Operation)> {
    [
        ("get", item.get.as_mut()),
        ("put", item.put.as_mut()),
        ("post", item.post.as_mut()),
        ("delete", item.delete.as_mut()),
        ("options", item.options.as_mut()),
        ("head", item.head.as_mut()),
        ("patch", item.patch.as_mut()),
        ("trace", item.trace.as_mut()),
    ]
    .into_iter()
    .filter_map(|(method, operation)| operation.map(|operation| (method, operation)))
    .collect()
}
