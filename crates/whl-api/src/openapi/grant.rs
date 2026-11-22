use utoipa::openapi::path::{Operation, Parameter, ParameterBuilder, ParameterIn};
use utoipa::openapi::{ObjectBuilder, OpenApi, Ref, RefOr, Required, Type};

use super::common::operations;

const GRANT_HEADER: &str = "X-Webhooks-Live-Grant";
const ENDPOINT_PARAMETER: &str = "{id}";
const UNLOCK_PATH: &str = "/endpoints/{id}/unlock";
const FOLDER_DETACH_PATH: &str = "/folders/{folder_id}/endpoints/{id}";
const GRANT_REQUIRED: &str = "; `grant_required`: a view-protected endpoint the caller does not own, without a valid `X-Webhooks-Live-Grant`";
const COMBINED_RESPONSE: &str = "UnauthenticatedOrGrantRequired";

pub(super) fn add_view_grant(document: &mut OpenApi) {
    for (path, item) in document.paths.paths.iter_mut() {
        if !path.contains(ENDPOINT_PARAMETER) {
            continue;
        }
        for (method, operation) in operations(item) {
            if is_exempt(method, path) {
                continue;
            }
            add_header(operation);
            add_response(operation);
        }
    }
}

fn is_exempt(method: &str, path: &str) -> bool {
    (method == "post" && path == UNLOCK_PATH) || (method == "delete" && path == FOLDER_DETACH_PATH)
}

fn add_header(operation: &mut Operation) {
    let parameters = operation.parameters.get_or_insert_with(Vec::new);
    if parameters
        .iter()
        .all(|parameter| parameter.name != GRANT_HEADER)
    {
        parameters.push(header_parameter());
    }
}

fn header_parameter() -> Parameter {
    ParameterBuilder::new()
        .name(GRANT_HEADER)
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .description(Some(
            "View grant of a view-protected endpoint the caller does not own, from `POST /endpoints/{id}/unlock`. Owners need none.",
        ))
        .schema(Some(ObjectBuilder::new().schema_type(Type::String)))
        .build()
}

fn add_response(operation: &mut Operation) {
    let responses = &mut operation.responses.responses;
    match responses.get_mut("401") {
        None => {
            responses.insert(
                "401".to_owned(),
                RefOr::Ref(Ref::from_response_name(COMBINED_RESPONSE)),
            );
        }
        Some(RefOr::Ref(reference)) if reference.ref_location.ends_with("/Unauthenticated") => {
            *reference = Ref::from_response_name(COMBINED_RESPONSE);
        }
        Some(RefOr::T(response)) if !response.description.contains("grant_required") => {
            response.description.push_str(GRANT_REQUIRED);
        }
        Some(_) => {}
    }
}
