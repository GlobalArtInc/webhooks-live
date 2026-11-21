use axum::Router;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{MatchedPath, RawPathParams, Request, State};
use axum::http::Method;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use whl_auth::{
    AccessControl, AccessMode, EndpointParam, EndpointRole, GrantRule, Requirement, RoutePolicy,
    TokenPolicy, TokenScope,
};

const API_PREFIX: &str = "/api/v1";
const ENDPOINT_PREFIX: &str = "/endpoints/{id}";
const ENDPOINT_PARAM: &str = "id";
const MEMBERS_TAIL: &str = "/members";
const CLAIM_TAIL: &str = "/claim";
const IMPORT_TAIL: &str = "/import";
const IMPORT_PATH: &str = "/endpoints/import";
const UNLOCK_TAIL: &str = "/unlock";
const STREAM_TAIL: &str = "/stream";
const FOLDER_ENDPOINT_PATH: &str = "/folders/{folder_id}/endpoints/{id}";
const TOKENS_PATH: &str = "/tokens";
const TOKEN_PATH: &str = "/tokens/{token_id}";
const USAGE_PATH: &str = "/usage";
const SCHEDULES_TAIL: &str = "/schedules";
const ALIAS_AVAILABILITY_PATH: &str = "/aliases/{alias}/availability";
const ALIASES_TAIL: &str = "/aliases";
const ALIAS_TAIL: &str = "/aliases/{alias}";

pub fn requirement(mode: AccessMode, method: &Method, pattern: &str) -> Requirement {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    match rest {
        "/instance" | "/inbox/captures" | "/inbox/stream" => Requirement::Anyone,
        "/email-addresses/verify" | "/notifications/disable" => Requirement::Anyone,
        "/email-addresses" | "/email-addresses/{address_id}" => Requirement::SignedIn,
        TOKENS_PATH | TOKEN_PATH | ALIAS_AVAILABILITY_PATH | USAGE_PATH => Requirement::SignedIn,
        "/send" | "/send/history" | "/send/history/{sent_id}" if mode == AccessMode::Sso => {
            Requirement::SignedIn
        }
        "/send" | "/send/history" | "/send/history/{sent_id}" => Requirement::Anyone,
        IMPORT_PATH => Requirement::SignedIn,
        "/endpoints" if mode == AccessMode::Mixed && *method == Method::POST => Requirement::Anyone,
        "/endpoints" => Requirement::SignedIn,
        "/folders" | "/folders/{folder_id}" => Requirement::SignedIn,
        "/templates" | "/templates/{template_id}" => Requirement::SignedIn,
        "/folders/{folder_id}/endpoints/{id}" if *method == Method::PUT => {
            Requirement::Role(EndpointRole::Viewer)
        }
        "/folders/{folder_id}/endpoints/{id}" => Requirement::SignedIn,
        "/push/public-key" => Requirement::Anyone,
        "/push/subscriptions" | "/push/subscriptions/{subscription_id}" => Requirement::SignedIn,
        _ => match rest.strip_prefix(ENDPOINT_PREFIX) {
            Some(CLAIM_TAIL) if *method == Method::POST => Requirement::SignedIn,
            Some(IMPORT_TAIL) if *method == Method::POST => {
                Requirement::OwnedRole(EndpointRole::Editor)
            }
            Some(UNLOCK_TAIL) if *method == Method::POST => Requirement::Anyone,
            Some(tail) if is_members_route(tail) => Requirement::OwnedRole(EndpointRole::Owner),
            Some(SCHEDULES_TAIL) if *method == Method::POST => {
                Requirement::OwnedRole(EndpointRole::Owner)
            }
            Some(ALIAS_TAIL) if matches!(*method, Method::PUT | Method::DELETE) => {
                Requirement::OwnedRole(EndpointRole::Owner)
            }
            Some(tail) => Requirement::Role(endpoint_role(method, tail)),
            None => Requirement::Role(EndpointRole::Owner),
        },
    }
}

pub fn token_policy(method: &Method, pattern: &str) -> TokenPolicy {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    let read = matches!(*method, Method::GET | Method::HEAD);
    let scope = match rest {
        "/instance" | USAGE_PATH if read => Some(TokenScope::Read),
        "/endpoints" if read => Some(TokenScope::Read),
        "/endpoints" if *method == Method::POST => Some(TokenScope::EndpointsWrite),
        "/send" if *method == Method::POST => Some(TokenScope::Send),
        "/send/history" if read => Some(TokenScope::Read),
        "/send/history/{sent_id}" if *method == Method::DELETE => Some(TokenScope::Send),
        ALIAS_AVAILABILITY_PATH if read => Some(TokenScope::Read),
        _ => rest
            .strip_prefix(ENDPOINT_PREFIX)
            .and_then(|tail| endpoint_scope(method, tail)),
    };
    scope.map_or(TokenPolicy::SessionOnly, TokenPolicy::Scope)
}

pub fn grant_rule(method: &Method, pattern: &str) -> GrantRule {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    let read = matches!(*method, Method::GET | Method::HEAD);
    if rest == FOLDER_ENDPOINT_PATH && *method == Method::DELETE {
        return GrantRule::NotRequired;
    }
    match rest.strip_prefix(ENDPOINT_PREFIX) {
        Some(UNLOCK_TAIL) if *method == Method::POST => GrantRule::NotRequired,
        Some(STREAM_TAIL) if read => GrantRule::HeaderOrQuery,
        _ => GrantRule::Header,
    }
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
        | "/captures/{capture_id}/parts/{index}"
        | "/forward-rules"
        | "/secrets"
        | ALIASES_TAIL
            if read =>
        {
            Some(TokenScope::Read)
        }
        "" => write(TokenScope::EndpointsWrite, &[Method::PATCH, Method::DELETE]),
        UNLOCK_TAIL => write(TokenScope::Read, &[Method::POST]),
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
        | "/stats"
        | "/captures"
        | "/captures/latest"
        | "/forward-rules"
        | "/rules"
        | "/rules/{rule_id}"
        | "/notifications"
        | "/captures/{capture_id}"
        | "/captures/{capture_id}/body"
        | "/captures/{capture_id}/parts/{index}"
        | ALIASES_TAIL
            if read =>
        {
            EndpointRole::Viewer
        }
        "/export" if read || *method == Method::POST => EndpointRole::Viewer,
        "/clone" if *method == Method::POST => EndpointRole::Viewer,
        "/forward-rules" | "/rules" | "/notifications" if *method == Method::POST => {
            EndpointRole::Editor
        }
        "/rules/order" if *method == Method::PUT => EndpointRole::Editor,
        "/rules/{rule_id}" if matches!(*method, Method::PATCH | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/notifications/{channel_id}" if matches!(*method, Method::PATCH | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/notifications/{channel_id}/test" if *method == Method::POST => EndpointRole::Editor,
        "/forward-rules/{rule_id}" if matches!(*method, Method::PATCH | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/captures" | "/captures/{capture_id}" if *method == Method::DELETE => EndpointRole::Editor,
        "/captures/{capture_id}" if *method == Method::PATCH => EndpointRole::Editor,
        "/schedules" | "/schedules/{schedule_id}/runs" if read => EndpointRole::Viewer,
        "/schedules/{schedule_id}" if matches!(*method, Method::PATCH | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/schedules/{schedule_id}/run" if *method == Method::POST => EndpointRole::Editor,
        "/secrets" if read => EndpointRole::Editor,
        "/secrets/{name}" if matches!(*method, Method::PUT | Method::DELETE) => {
            EndpointRole::Editor
        }
        "/vars" if read => EndpointRole::Viewer,
        "/vars/{name}" if matches!(*method, Method::PUT | Method::DELETE) => EndpointRole::Editor,
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
    let grant = grant_rule(request.method(), pattern);
    let endpoint = EndpointParam::from_path(&params, ENDPOINT_PARAM);
    let route = RoutePolicy {
        requirement,
        token,
        grant,
    };
    access.enforce_route(route, endpoint, request, next).await
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
        let table: [(&Method, &str, Requirement); 48] = [
            (&GET, "/vars", Requirement::Role(Viewer)),
            (&Method::HEAD, "/vars", Requirement::Role(Viewer)),
            (&PUT, "/vars/{name}", Requirement::Role(Editor)),
            (&DELETE, "/vars/{name}", Requirement::Role(Editor)),
            (&GET, "/vars/{name}", Requirement::Role(Owner)),
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
            (&GET, "/rules", Requirement::Role(Viewer)),
            (&POST, "/rules", Requirement::Role(Editor)),
            (&GET, "/rules/{rule_id}", Requirement::Role(Viewer)),
            (&PATCH, "/rules/{rule_id}", Requirement::Role(Editor)),
            (&DELETE, "/rules/{rule_id}", Requirement::Role(Editor)),
            (&PUT, "/rules/order", Requirement::Role(Editor)),
            (&GET, "/notifications", Requirement::Role(Viewer)),
            (&POST, "/notifications", Requirement::Role(Editor)),
            (
                &PATCH,
                "/notifications/{channel_id}",
                Requirement::Role(Editor),
            ),
            (
                &POST,
                "/notifications/{channel_id}/test",
                Requirement::Role(Editor),
            ),
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
            (
                &GET,
                "/captures/{capture_id}/parts/{index}",
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
            (&GET, "/stats", Requirement::Role(Viewer)),
            (&PATCH, "/captures/{capture_id}", Requirement::Role(Editor)),
            (&GET, "/export", Requirement::Role(Viewer)),
            (&POST, "/export", Requirement::Role(Viewer)),
            (&Method::HEAD, "/export", Requirement::Role(Viewer)),
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
    fn clone_needs_a_viewer_and_template_routes_need_a_signed_in_caller() {
        for mode in MODES {
            assert_eq!(
                requirement(mode, &POST, "/api/v1/endpoints/{id}/clone"),
                Requirement::Role(EndpointRole::Viewer)
            );
            assert_eq!(
                requirement(mode, &GET, "/api/v1/endpoints/{id}/clone"),
                Requirement::Role(EndpointRole::Owner)
            );
            for pattern in ["/api/v1/templates", "/api/v1/templates/{template_id}"] {
                for method in [&GET, &POST, &PATCH, &DELETE] {
                    assert_eq!(
                        requirement(mode, method, pattern),
                        Requirement::SignedIn,
                        "{mode:?} {method} {pattern}"
                    );
                }
            }
        }
    }

    #[test]
    fn schedule_routes_need_the_roles_of_the_api_table() {
        use EndpointRole::{Editor, Owner, Viewer};
        let table: [(&Method, &str, Requirement); 8] = [
            (&GET, "/schedules", Requirement::Role(Viewer)),
            (&POST, "/schedules", Requirement::OwnedRole(Owner)),
            (
                &PATCH,
                "/schedules/{schedule_id}",
                Requirement::Role(Editor),
            ),
            (
                &DELETE,
                "/schedules/{schedule_id}",
                Requirement::Role(Editor),
            ),
            (
                &POST,
                "/schedules/{schedule_id}/run",
                Requirement::Role(Editor),
            ),
            (
                &GET,
                "/schedules/{schedule_id}/runs",
                Requirement::Role(Viewer),
            ),
            (&GET, "/schedules/{schedule_id}", Requirement::Role(Owner)),
            (&PUT, "/schedules", Requirement::Role(Owner)),
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
        assert_eq!(role(&PUT, "/captures/{capture_id}"), owner);
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
    fn the_composer_routes_need_sign_in_only_in_sso_mode() {
        for pattern in [
            "/api/v1/send",
            "/api/v1/send/history",
            "/api/v1/send/history/{sent_id}",
        ] {
            for method in [&GET, &POST, &DELETE] {
                assert_eq!(
                    requirement(AccessMode::Sso, method, pattern),
                    Requirement::SignedIn,
                    "{pattern}"
                );
                for mode in [AccessMode::Open, AccessMode::Mixed] {
                    assert_eq!(
                        requirement(mode, method, pattern),
                        Requirement::Anyone,
                        "{mode:?} {pattern}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_push_key_is_public_and_the_subscription_routes_need_sign_in_in_every_mode() {
        for mode in MODES {
            assert_eq!(
                requirement(mode, &GET, "/api/v1/push/public-key"),
                Requirement::Anyone
            );
            for (method, pattern) in [
                (&GET, "/api/v1/push/subscriptions"),
                (&POST, "/api/v1/push/subscriptions"),
                (&DELETE, "/api/v1/push/subscriptions/{subscription_id}"),
            ] {
                assert_eq!(
                    requirement(mode, method, pattern),
                    Requirement::SignedIn,
                    "{mode:?} {method} {pattern}"
                );
            }
        }
    }

    #[test]
    fn the_composer_routes_map_to_the_send_and_read_token_scopes() {
        assert_eq!(
            token_policy(&POST, "/api/v1/send"),
            TokenPolicy::Scope(TokenScope::Send)
        );
        assert_eq!(
            token_policy(&GET, "/api/v1/send/history"),
            TokenPolicy::Scope(TokenScope::Read)
        );
        assert_eq!(
            token_policy(&DELETE, "/api/v1/send/history/{sent_id}"),
            TokenPolicy::Scope(TokenScope::Send)
        );
        assert_eq!(token_policy(&GET, "/api/v1/send"), TokenPolicy::SessionOnly);
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
    fn folder_routes_need_sign_in_and_the_viewer_role_only_to_join_an_endpoint() {
        for mode in MODES {
            assert_eq!(
                requirement(mode, &GET, "/api/v1/folders"),
                Requirement::SignedIn
            );
            assert_eq!(
                requirement(mode, &POST, "/api/v1/folders"),
                Requirement::SignedIn
            );
            assert_eq!(
                requirement(mode, &PATCH, "/api/v1/folders/{folder_id}"),
                Requirement::SignedIn
            );
            assert_eq!(
                requirement(mode, &DELETE, "/api/v1/folders/{folder_id}"),
                Requirement::SignedIn
            );
            assert_eq!(
                requirement(mode, &PUT, "/api/v1/folders/{folder_id}/endpoints/{id}"),
                Requirement::Role(EndpointRole::Viewer)
            );
            assert_eq!(
                requirement(mode, &DELETE, "/api/v1/folders/{folder_id}/endpoints/{id}"),
                Requirement::SignedIn
            );
        }
    }

    #[test]
    fn the_inbox_routes_pass_the_gate_and_are_checked_per_endpoint_by_the_handler() {
        for mode in MODES {
            for route in ["/api/v1/inbox/captures", "/api/v1/inbox/stream"] {
                assert_eq!(requirement(mode, &GET, route), Requirement::Anyone);
            }
        }
    }

    #[test]
    fn importing_needs_sign_in_for_a_new_endpoint_and_an_owned_editor_role_for_an_existing_one() {
        for mode in MODES {
            for method in [&POST, &GET, &PUT, &DELETE] {
                assert_eq!(
                    requirement(mode, method, "/api/v1/endpoints/import"),
                    Requirement::SignedIn
                );
            }
            assert_eq!(
                requirement(mode, &POST, "/api/v1/endpoints/{id}/import"),
                Requirement::OwnedRole(EndpointRole::Editor)
            );
            assert_eq!(
                requirement(mode, &GET, "/api/v1/endpoints/{id}/import"),
                Requirement::Role(EndpointRole::Owner)
            );
        }
    }

    #[test]
    fn the_alias_routes_need_the_owner_of_an_owned_endpoint_to_change_and_refuse_tokens_for_writes()
    {
        use EndpointRole::{Owner, Viewer};
        for mode in MODES {
            for (method, tail, expected) in [
                (&GET, "/aliases", Requirement::Role(Viewer)),
                (&PUT, "/aliases/{alias}", Requirement::OwnedRole(Owner)),
                (&DELETE, "/aliases/{alias}", Requirement::OwnedRole(Owner)),
                (&GET, "/aliases/{alias}", Requirement::Role(Owner)),
                (&POST, "/aliases", Requirement::Role(Owner)),
            ] {
                let pattern = format!("/api/v1/endpoints/{{id}}{tail}");
                assert_eq!(
                    requirement(mode, method, &pattern),
                    expected,
                    "{mode:?} {method} {tail}"
                );
            }
            assert_eq!(
                requirement(mode, &GET, "/api/v1/aliases/{alias}/availability"),
                Requirement::SignedIn
            );
        }
        for (method, pattern, expected) in [
            (
                &GET,
                "/api/v1/endpoints/{id}/aliases",
                TokenPolicy::Scope(TokenScope::Read),
            ),
            (
                &GET,
                "/api/v1/aliases/{alias}/availability",
                TokenPolicy::Scope(TokenScope::Read),
            ),
            (
                &PUT,
                "/api/v1/endpoints/{id}/aliases/{alias}",
                TokenPolicy::SessionOnly,
            ),
            (
                &DELETE,
                "/api/v1/endpoints/{id}/aliases/{alias}",
                TokenPolicy::SessionOnly,
            ),
        ] {
            assert_eq!(
                token_policy(method, pattern),
                expected,
                "{method} {pattern}"
            );
        }
    }

    #[test]
    fn the_email_routes_need_sign_in_except_the_two_token_routes() {
        for mode in MODES {
            for (method, pattern, expected) in [
                (&GET, "/api/v1/email-addresses", Requirement::SignedIn),
                (&POST, "/api/v1/email-addresses", Requirement::SignedIn),
                (
                    &DELETE,
                    "/api/v1/email-addresses/{address_id}",
                    Requirement::SignedIn,
                ),
                (&POST, "/api/v1/email-addresses/verify", Requirement::Anyone),
                (&POST, "/api/v1/notifications/disable", Requirement::Anyone),
                (&GET, "/api/v1/notifications/disable", Requirement::Anyone),
                (&GET, "/api/v1/email-addresses/verify", Requirement::Anyone),
                (
                    &PATCH,
                    "/api/v1/email-addresses/{address_id}",
                    Requirement::SignedIn,
                ),
            ] {
                assert_eq!(
                    requirement(mode, method, pattern),
                    expected,
                    "{mode:?} {method} {pattern}"
                );
            }
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
    fn unlock_is_open_to_anyone_with_the_read_scope_and_needs_no_grant() {
        let pattern = "/api/v1/endpoints/{id}/unlock";
        for mode in MODES {
            assert_eq!(requirement(mode, &POST, pattern), Requirement::Anyone);
        }
        assert_eq!(
            token_policy(&POST, pattern),
            TokenPolicy::Scope(TokenScope::Read)
        );
        assert_eq!(grant_rule(&POST, pattern), GrantRule::NotRequired);
        assert_eq!(token_policy(&GET, pattern), TokenPolicy::SessionOnly);
        assert_eq!(grant_rule(&GET, pattern), GrantRule::Header);
        assert_eq!(
            role(&GET, "/unlock"),
            Requirement::Role(EndpointRole::Owner)
        );
    }

    #[test]
    fn only_the_stream_takes_the_grant_in_the_query_and_every_other_route_needs_the_header() {
        let stream = "/api/v1/endpoints/{id}/stream";
        assert_eq!(grant_rule(&GET, stream), GrantRule::HeaderOrQuery);
        assert_eq!(grant_rule(&Method::HEAD, stream), GrantRule::HeaderOrQuery);
        assert_eq!(grant_rule(&POST, stream), GrantRule::Header);
        for (method, tail) in [
            (&GET, ""),
            (&PATCH, ""),
            (&DELETE, ""),
            (&GET, "/captures"),
            (&GET, "/captures/latest"),
            (&GET, "/captures/{capture_id}/body"),
            (&POST, "/captures/{capture_id}/resend"),
            (&POST, "/claim"),
            (&PUT, "/members"),
            (&GET, "/secrets"),
            (&GET, "/stream-extra"),
            (&GET, "/something-new"),
        ] {
            let pattern = format!("/api/v1/endpoints/{{id}}{tail}");
            assert_eq!(
                grant_rule(method, &pattern),
                GrantRule::Header,
                "{method} {tail}"
            );
        }
        assert_eq!(
            grant_rule(&PUT, "/api/v1/folders/{folder_id}/endpoints/{id}"),
            GrantRule::Header
        );
        assert_eq!(
            grant_rule(&DELETE, "/api/v1/folders/{folder_id}/endpoints/{id}"),
            GrantRule::NotRequired
        );
        assert_eq!(grant_rule(&GET, ""), GrantRule::Header);
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
        let table: [(&Method, &str, TokenPolicy); 31] = [
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
                &GET,
                "/api/v1/endpoints/{id}/captures/{capture_id}/parts/{index}",
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
            (&POST, "/api/v1/endpoints/import"),
            (&POST, "/api/v1/endpoints/{id}/import"),
        ] {
            assert_eq!(
                token_policy(method, pattern),
                TokenPolicy::SessionOnly,
                "{method} {pattern}"
            );
        }
    }
}
