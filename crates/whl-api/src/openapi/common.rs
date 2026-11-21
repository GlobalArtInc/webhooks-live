use utoipa::openapi::path::{Operation, PathItem};
use utoipa::openapi::{OpenApi, Ref, RefOr};

const COMMON: [(&str, &str); 2] = [("429", "RateLimited"), ("500", "InternalError")];

pub(super) fn add_common_responses(document: &mut OpenApi) {
    for item in document.paths.paths.values_mut() {
        for operation in operations(item) {
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

fn operations(item: &mut PathItem) -> Vec<&mut Operation> {
    [
        item.get.as_mut(),
        item.put.as_mut(),
        item.post.as_mut(),
        item.delete.as_mut(),
        item.options.as_mut(),
        item.head.as_mut(),
        item.patch.as_mut(),
        item.trace.as_mut(),
    ]
    .into_iter()
    .flatten()
    .collect()
}
