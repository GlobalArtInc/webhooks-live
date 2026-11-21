use axum::Router;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{MatchedPath, RawPathParams, Request, State};
use axum::http::Method;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use whl_auth::{AccessControl, AccessMode, EndpointParam, EndpointRole, Requirement};

const API_PREFIX: &str = "/api/v1";
const ENDPOINT_PREFIX: &str = "/endpoints/{id}";
const ENDPOINT_PARAM: &str = "id";
const MEMBERS_TAIL: &str = "/members";
const CLAIM_TAIL: &str = "/claim";

pub fn requirement(mode: AccessMode, method: &Method, pattern: &str) -> Requirement {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    match rest {
        "/instance" => Requirement::Anyone,
        "/endpoints" if mode == AccessMode::Mixed && *method == Method::POST => Requirement::Anyone,
        "/endpoints" => Requirement::SignedIn,
        _ => match rest.strip_prefix(ENDPOINT_PREFIX) {
            Some(CLAIM_TAIL) if *method == Method::POST => Requirement::SignedIn,
            Some(tail) if is_members_route(tail) => Requirement::OwnedRole(EndpointRole::Owner),
            Some(tail) => Requirement::Role(endpoint_role(method, tail)),
            None => Requirement::Role(EndpointRole::Owner),
        },
    }
}

fn is_members_route(tail: &str) -> bool {
    tail.strip_prefix(MEMBERS_TAIL)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn endpoint_role(method: &Method, tail: &str) -> EndpointRole {
    let read = matches!(*method, Method::GET | Method::HEAD);
    match tail {
        "" if read => EndpointRole::Viewer,
        "" if *method == Method::PATCH => EndpointRole::Editor,
        "/stream"
        | "/captures"
        | "/captures/latest"
        | "/forward-rules"
        | "/captures/{capture_id}"
        | "/captures/{capture_id}/body"
            if read =>
        {
            EndpointRole::Viewer
        }
        "/forward-rules" if *method == Method::POST => EndpointRole::Editor,
        "/forward-rules/{rule_id}" if matches!(*method, Method::PATCH | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/captures" | "/captures/{capture_id}" if *method == Method::DELETE => EndpointRole::Editor,
        "/secrets" if read => EndpointRole::Editor,
        "/secrets/{name}" if matches!(*method, Method::PUT | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/captures/{capture_id}/forward" | "/captures/{capture_id}/resend"
            if *method == Method::POST =>
        {
            EndpointRole::Editor
        }
        _ => EndpointRole::Owner,
    }
}

async fn guard(
    State(access): State<AccessControl>,
    matched: Option<MatchedPath>,
    params: Result<RawPathParams, RawPathParamsRejection>,
    request: Request,
    next: Next,
) -> Response {
    let pattern = matched.as_ref().map_or("", MatchedPath::as_str);
    let requirement = requirement(access.mode(), request.method(), pattern);
    let endpoint = EndpointParam::from_path(&params, ENDPOINT_PARAM);
    access.enforce(requirement, endpoint, request, next).await
}

pub fn guard_routes<S>(routes: Router<S>, access: &AccessControl) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    routes.route_layer(from_fn_with_state(access.clone(), guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GET: Method = Method::GET;
    const POST: Method = Method::POST;
    const PATCH: Method = Method::PATCH;
    const PUT: Method = Method::PUT;
    const DELETE: Method = Method::DELETE;
    const MODES: [AccessMode; 3] = [AccessMode::Open, AccessMode::Sso, AccessMode::Mixed];

    fn role(method: &Method, tail: &str) -> Requirement {
        requirement(
            AccessMode::Mixed,
            method,
            &format!("/api/v1/endpoints/{{id}}{tail}"),
        )
    }

    #[test]
    fn every_documented_route_needs_the_role_of_the_api_table() {
        use EndpointRole::{Editor, Owner, Viewer};
        let table: [(&Method, &str, Requirement); 27] = [
            (&GET, "/secrets", Requirement::Role(Editor)),
            (&Method::HEAD, "/secrets", Requirement::Role(Editor)),
            (&PUT, "/secrets/{name}", Requirement::Role(Editor)),
            (&DELETE, "/secrets/{name}", Requirement::Role(Editor)),
            (&GET, "", Requirement::Role(Viewer)),
            (&PATCH, "", Requirement::Role(Editor)),
            (&DELETE, "", Requirement::Role(Owner)),
            (&GET, "/members", Requirement::OwnedRole(Owner)),
            (&PUT, "/members", Requirement::OwnedRole(Owner)),
            (&DELETE, "/members/{user_id}", Requirement::OwnedRole(Owner)),
            (&GET, "/forward-rules", Requirement::Role(Viewer)),
            (&POST, "/forward-rules", Requirement::Role(Editor)),
            (
                &PATCH,
                "/forward-rules/{rule_id}",
                Requirement::Role(Editor),
            ),
            (
                &DELETE,
                "/forward-rules/{rule_id}",
                Requirement::Role(Editor),
            ),
            (&GET, "/captures", Requirement::Role(Viewer)),
            (&GET, "/captures/latest", Requirement::Role(Viewer)),
            (&GET, "/captures/{capture_id}", Requirement::Role(Viewer)),
            (
                &GET,
                "/captures/{capture_id}/body",
                Requirement::Role(Viewer),
            ),
            (&DELETE, "/captures/{capture_id}", Requirement::Role(Editor)),
            (&DELETE, "/captures", Requirement::Role(Editor)),
            (
                &POST,
                "/captures/{capture_id}/forward",
                Requirement::Role(Editor),
            ),
            (
                &POST,
                "/captures/{capture_id}/resend",
                Requirement::Role(Editor),
            ),
            (&GET, "/stream", Requirement::Role(Viewer)),
            (&Method::HEAD, "", Requirement::Role(Viewer)),
            (&Method::HEAD, "/captures", Requirement::Role(Viewer)),
            (&Method::HEAD, "/stream", Requirement::Role(Viewer)),
            (
                &Method::HEAD,
                "/captures/{capture_id}/body",
                Requirement::Role(Viewer),
            ),
        ];
        for mode in MODES {
            for (method, tail, expected) in &table {
                let pattern = format!("/api/v1/endpoints/{{id}}{tail}");
                assert_eq!(
                    requirement(mode, method, &pattern),
                    *expected,
                    "{mode:?} {method} {tail}"
                );
            }
        }
    }

    #[test]
    fn an_unlisted_endpoint_route_or_method_needs_the_owner() {
        let owner = Requirement::Role(EndpointRole::Owner);
        assert_eq!(role(&GET, "/something-new"), owner);
        assert_eq!(role(&POST, "/something-new"), owner);
        assert_eq!(role(&PUT, ""), owner);
        assert_eq!(role(&POST, ""), owner);
        assert_eq!(role(&PUT, "/captures"), owner);
        assert_eq!(role(&POST, "/captures"), owner);
        assert_eq!(role(&GET, "/forward-rules/{rule_id}"), owner);
        assert_eq!(role(&PATCH, "/captures/{capture_id}"), owner);
        assert_eq!(role(&PATCH, "/stream"), owner);
        assert_eq!(role(&GET, "/claim"), owner);
        assert_eq!(role(&POST, "/secrets"), owner);
        assert_eq!(role(&GET, "/secrets/{name}"), owner);
        assert_eq!(role(&PATCH, "/secrets/{name}"), owner);
        assert_eq!(role(&GET, "/members-extra"), owner);
        assert_eq!(
            role(&Method::from_bytes(b"get").expect("method"), ""),
            owner
        );
    }

    #[test]
    fn the_listing_needs_sign_in_the_instance_is_open_and_an_unknown_pattern_needs_the_owner() {
        for mode in MODES {
            assert_eq!(
                requirement(mode, &GET, "/api/v1/instance"),
                Requirement::Anyone
            );
            assert_eq!(
                requirement(mode, &GET, "/api/v1/endpoints"),
                Requirement::SignedIn
            );
            for pattern in ["/api/v1/tokens", "", "/", "/api/v1", "/api/v1/endpointsx"] {
                assert_eq!(
                    requirement(mode, &GET, pattern),
                    Requirement::Role(EndpointRole::Owner),
                    "{mode:?} {pattern:?}"
                );
            }
        }
    }

    #[test]
    fn creating_an_endpoint_is_open_to_anyone_only_in_mixed_mode() {
        assert_eq!(
            requirement(AccessMode::Mixed, &POST, "/api/v1/endpoints"),
            Requirement::Anyone
        );
        for mode in [AccessMode::Open, AccessMode::Sso] {
            assert_eq!(
                requirement(mode, &POST, "/api/v1/endpoints"),
                Requirement::SignedIn
            );
        }
    }

    #[test]
    fn the_claim_route_needs_a_signed_in_caller_in_every_mode() {
        for mode in MODES {
            assert_eq!(
                requirement(mode, &POST, "/api/v1/endpoints/{id}/claim"),
                Requirement::SignedIn
            );
        }
    }
}
