use axum::Router;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{MatchedPath, RawPathParams, Request, State};
use axum::http::Method;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use whl_auth::{
    AccessControl, AccessMode, EndpointParam, EndpointRole, Requirement, TokenPolicy, TokenScope,
};

const API_PREFIX: &str = "/api/v1";
const ENDPOINT_PREFIX: &str = "/endpoints/{id}";
const ENDPOINT_PARAM: &str = "id";
const MEMBERS_TAIL: &str = "/members";
const CLAIM_TAIL: &str = "/claim";
const TOKENS_PATH: &str = "/tokens";
const TOKEN_PATH: &str = "/tokens/{token_id}";

pub fn requirement(mode: AccessMode, method: &Method, pattern: &str) -> Requirement {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    match rest {
        "/instance" => Requirement::Anyone,
        TOKENS_PATH | TOKEN_PATH => Requirement::SignedIn,
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

pub fn token_policy(method: &Method, pattern: &str) -> TokenPolicy {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    let read = matches!(*method, Method::GET | Method::HEAD);
    let scope = match rest {
        "/instance" if read => Some(TokenScope::Read),
        "/endpoints" if read => Some(TokenScope::Read),
        "/endpoints" if *method == Method::POST => Some(TokenScope::EndpointsWrite),
        _ => rest
            .strip_prefix(ENDPOINT_PREFIX)
            .and_then(|tail| endpoint_scope(method, tail)),
    };
    scope.map_or(TokenPolicy::SessionOnly, TokenPolicy::Scope)
}

fn endpoint_scope(method: &Method, tail: &str) -> Option<TokenScope> {
    let read = matches!(*method, Method::GET | Method::HEAD);
    let write = |scope: TokenScope, methods: &[Method]| methods.contains(method).then_some(scope);
    match tail {
        "" if read => Some(TokenScope::Read),
        "/stream"
        | "/captures"
        | "/captures/latest"
        | "/captures/{capture_id}"
        | "/captures/{capture_id}/body"
        | "/forward-rules"
        | "/secrets"
            if read =>
        {
            Some(TokenScope::Read)
        }
        "" => write(TokenScope::EndpointsWrite, &[Method::PATCH, Method::DELETE]),
        "/secrets/{name}" => write(TokenScope::EndpointsWrite, &[Method::PUT, Method::DELETE]),
        "/captures" | "/captures/{capture_id}" => {
            write(TokenScope::CapturesWrite, &[Method::DELETE])
        }
        "/forward-rules" => write(TokenScope::RulesWrite, &[Method::POST]),
        "/forward-rules/{rule_id}" => {
            write(TokenScope::RulesWrite, &[Method::PATCH, Method::DELETE])
        }
        "/captures/{capture_id}/forward" | "/captures/{capture_id}/resend" => {
            write(TokenScope::Send, &[Method::POST])
        }
        _ => None,
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
    let token = token_policy(request.method(), pattern);
    let endpoint = EndpointParam::from_path(&params, ENDPOINT_PARAM);
    access
        .enforce_for(requirement, token, endpoint, request, next)
        .await
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
            for pattern in ["", "/", "/api/v1", "/api/v1/endpointsx", "/api/v1/tokensx"] {
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

    #[test]
    fn the_token_routes_need_sign_in_in_every_mode() {
        for mode in MODES {
            for (method, pattern) in [
                (&GET, "/api/v1/tokens"),
                (&POST, "/api/v1/tokens"),
                (&DELETE, "/api/v1/tokens/{token_id}"),
            ] {
                assert_eq!(
                    requirement(mode, method, pattern),
                    Requirement::SignedIn,
                    "{mode:?} {method} {pattern}"
                );
                assert_eq!(
                    token_policy(method, pattern),
                    TokenPolicy::SessionOnly,
                    "{method} {pattern}"
                );
            }
        }
    }

    #[test]
    fn every_documented_route_maps_to_the_token_scope_of_the_table() {
        use TokenScope::{CapturesWrite, EndpointsWrite, Read, RulesWrite, Send};
        let table: [(&Method, &str, TokenPolicy); 30] = [
            (&GET, "/api/v1/instance", TokenPolicy::Scope(Read)),
            (&GET, "/api/v1/endpoints", TokenPolicy::Scope(Read)),
            (
                &POST,
                "/api/v1/endpoints",
                TokenPolicy::Scope(EndpointsWrite),
            ),
            (&GET, "/api/v1/endpoints/{id}", TokenPolicy::Scope(Read)),
            (
                &Method::HEAD,
                "/api/v1/endpoints/{id}",
                TokenPolicy::Scope(Read),
            ),
            (
                &PATCH,
                "/api/v1/endpoints/{id}",
                TokenPolicy::Scope(EndpointsWrite),
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}",
                TokenPolicy::Scope(EndpointsWrite),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/stream",
                TokenPolicy::Scope(Read),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/captures",
                TokenPolicy::Scope(Read),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/captures/latest",
                TokenPolicy::Scope(Read),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/captures/{capture_id}",
                TokenPolicy::Scope(Read),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/captures/{capture_id}/body",
                TokenPolicy::Scope(Read),
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/captures",
                TokenPolicy::Scope(CapturesWrite),
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/captures/{capture_id}",
                TokenPolicy::Scope(CapturesWrite),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/forward-rules",
                TokenPolicy::Scope(Read),
            ),
            (
                &POST,
                "/api/v1/endpoints/{id}/forward-rules",
                TokenPolicy::Scope(RulesWrite),
            ),
            (
                &PATCH,
                "/api/v1/endpoints/{id}/forward-rules/{rule_id}",
                TokenPolicy::Scope(RulesWrite),
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/forward-rules/{rule_id}",
                TokenPolicy::Scope(RulesWrite),
            ),
            (
                &POST,
                "/api/v1/endpoints/{id}/captures/{capture_id}/forward",
                TokenPolicy::Scope(Send),
            ),
            (
                &POST,
                "/api/v1/endpoints/{id}/captures/{capture_id}/resend",
                TokenPolicy::Scope(Send),
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/secrets",
                TokenPolicy::Scope(Read),
            ),
            (
                &PUT,
                "/api/v1/endpoints/{id}/secrets/{name}",
                TokenPolicy::Scope(EndpointsWrite),
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/secrets/{name}",
                TokenPolicy::Scope(EndpointsWrite),
            ),
            (
                &POST,
                "/api/v1/endpoints/{id}/claim",
                TokenPolicy::SessionOnly,
            ),
            (
                &GET,
                "/api/v1/endpoints/{id}/members",
                TokenPolicy::SessionOnly,
            ),
            (
                &PUT,
                "/api/v1/endpoints/{id}/members",
                TokenPolicy::SessionOnly,
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/members/{user_id}",
                TokenPolicy::SessionOnly,
            ),
            (&GET, "/api/v1/tokens", TokenPolicy::SessionOnly),
            (&POST, "/api/v1/tokens", TokenPolicy::SessionOnly),
            (
                &DELETE,
                "/api/v1/tokens/{token_id}",
                TokenPolicy::SessionOnly,
            ),
        ];
        for (method, pattern, expected) in &table {
            assert_eq!(
                token_policy(method, pattern),
                *expected,
                "{method} {pattern}"
            );
        }
    }

    #[test]
    fn an_unlisted_route_or_method_refuses_tokens() {
        for (method, pattern) in [
            (&GET, "/api/v1/endpoints/{id}/something-new"),
            (&POST, "/api/v1/endpoints/{id}"),
            (&PUT, "/api/v1/endpoints/{id}/captures"),
            (&PATCH, "/api/v1/endpoints/{id}/captures/{capture_id}"),
            (&GET, "/api/v1/endpoints/{id}/forward-rules/{rule_id}"),
            (&POST, "/api/v1/instance"),
            (&DELETE, "/api/v1/endpoints"),
            (&GET, "/api/v1/unknown"),
            (&GET, ""),
        ] {
            assert_eq!(
                token_policy(method, pattern),
                TokenPolicy::SessionOnly,
                "{method} {pattern}"
            );
        }
    }
}
