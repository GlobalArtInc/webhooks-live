use axum::Router;
use axum::extract::rejection::RawPathParamsRejection;
use axum::extract::{MatchedPath, RawPathParams, Request, State};
use axum::http::Method;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::Response;
use whl_auth::{AccessControl, EndpointRole, Requirement};

const API_PREFIX: &str = "/api/v1";
const ENDPOINT_PREFIX: &str = "/endpoints/{id}";
const ENDPOINT_PARAM: &str = "id";

pub fn requirement(method: &Method, pattern: &str) -> Requirement {
    let rest = pattern.strip_prefix(API_PREFIX).unwrap_or(pattern);
    match rest {
        "/instance" => Requirement::Anyone,
        "/endpoints" => Requirement::SignedIn,
        _ => match rest.strip_prefix(ENDPOINT_PREFIX) {
            Some(tail) => Requirement::Role(endpoint_role(method, tail)),
            None => Requirement::SignedIn,
        },
    }
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
    let requirement = requirement(request.method(), pattern);
    let endpoint = params.as_ref().ok().and_then(|params| {
        params
            .iter()
            .find(|(name, _)| *name == ENDPOINT_PARAM)
            .map(|(_, value)| value.to_owned())
    });
    access
        .enforce(requirement, endpoint.as_deref(), request, next)
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

    fn role(method: &Method, tail: &str) -> Requirement {
        requirement(method, &format!("/api/v1/endpoints/{{id}}{tail}"))
    }

    #[test]
    fn every_documented_route_needs_the_role_of_the_api_table() {
        use EndpointRole::{Editor, Owner, Viewer};
        let table: [(&Method, &str, EndpointRole); 23] = [
            (&GET, "", Viewer),
            (&PATCH, "", Editor),
            (&DELETE, "", Owner),
            (&GET, "/members", Owner),
            (&PUT, "/members", Owner),
            (&DELETE, "/members/{user_id}", Owner),
            (&GET, "/forward-rules", Viewer),
            (&POST, "/forward-rules", Editor),
            (&PATCH, "/forward-rules/{rule_id}", Editor),
            (&DELETE, "/forward-rules/{rule_id}", Editor),
            (&GET, "/captures", Viewer),
            (&GET, "/captures/latest", Viewer),
            (&GET, "/captures/{capture_id}", Viewer),
            (&GET, "/captures/{capture_id}/body", Viewer),
            (&DELETE, "/captures/{capture_id}", Editor),
            (&DELETE, "/captures", Editor),
            (&POST, "/captures/{capture_id}/forward", Editor),
            (&POST, "/captures/{capture_id}/resend", Editor),
            (&GET, "/stream", Viewer),
            (&Method::HEAD, "", Viewer),
            (&Method::HEAD, "/captures", Viewer),
            (&Method::HEAD, "/stream", Viewer),
            (&Method::HEAD, "/captures/{capture_id}/body", Viewer),
        ];
        for (method, tail, expected) in table {
            assert_eq!(
                role(method, tail),
                Requirement::Role(expected),
                "{method} {tail}"
            );
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
        assert_eq!(
            role(&Method::from_bytes(b"get").expect("method"), ""),
            owner
        );
    }

    #[test]
    fn routes_without_an_endpoint_need_sign_in_except_the_instance() {
        assert_eq!(requirement(&GET, "/api/v1/instance"), Requirement::Anyone);
        assert_eq!(
            requirement(&GET, "/api/v1/endpoints"),
            Requirement::SignedIn
        );
        assert_eq!(
            requirement(&POST, "/api/v1/endpoints"),
            Requirement::SignedIn
        );
        assert_eq!(requirement(&GET, "/api/v1/tokens"), Requirement::SignedIn);
        assert_eq!(requirement(&GET, ""), Requirement::SignedIn);
    }
}
