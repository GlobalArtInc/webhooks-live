use axum::http::Uri;
use whl_core::alias::Alias;
use whl_core::endpoint_id::EndpointId;
use whl_store::AliasStoreError;

use crate::refusal::Refusal;
use crate::state::CaptureState;

const ALIAS_PREFIX: &str = "/a/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target<'a> {
    Endpoint(EndpointId),
    Alias(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Routed<'a> {
    pub target: Target<'a>,
    pub sub_path: &'a str,
}

pub(crate) fn route(uri: &Uri) -> Option<Routed<'_>> {
    let path = uri.path();
    let rest = path.strip_prefix('/')?;
    let first = rest.split('/').next()?;
    if let Ok(id) = EndpointId::parse(first) {
        return Some(Routed {
            target: Target::Endpoint(id),
            sub_path: &path[1 + first.len()..],
        });
    }
    let after_prefix = path.strip_prefix(ALIAS_PREFIX)?;
    let name = after_prefix.split('/').next()?;
    if name.is_empty() {
        return None;
    }
    Some(Routed {
        target: Target::Alias(name),
        sub_path: &after_prefix[name.len()..],
    })
}

pub(crate) async fn resolve(
    state: &CaptureState,
    target: Target<'_>,
) -> Result<EndpointId, Refusal> {
    let name = match target {
        Target::Endpoint(id) => return Ok(id),
        Target::Alias(name) => name,
    };
    let Ok(alias) = Alias::parse(name) else {
        return Err(Refusal::Gone(None));
    };
    match state.resolve_alias(&alias).await {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(Refusal::Gone(None)),
        Err(error) => Err(store_refusal(&error)),
    }
}

fn store_refusal(error: &AliasStoreError) -> Refusal {
    if error.is_unavailable() {
        tracing::warn!(%error, "the store is unavailable for an alias lookup");
        Refusal::StoreUnavailable
    } else {
        tracing::error!(%error, "the store failed an alias lookup");
        Refusal::Internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

    fn uri(path: &str) -> Uri {
        path.parse().expect("a valid uri")
    }

    #[test]
    fn an_endpoint_id_routes_with_the_rest_of_the_path() {
        let expected = EndpointId::parse(ID).expect("a valid id");
        for (path, sub_path) in [
            (format!("/{ID}"), ""),
            (format!("/{ID}/"), "/"),
            (format!("/{ID}/404"), "/404"),
            (format!("/{ID}/a/b"), "/a/b"),
        ] {
            let uri = uri(&path);
            let routed = route(&uri).expect("routes");
            assert_eq!(routed.target, Target::Endpoint(expected.clone()), "{path}");
            assert_eq!(routed.sub_path, sub_path, "{path}");
        }
    }

    #[test]
    fn an_alias_routes_with_the_rest_of_the_path() {
        for (path, name, sub_path) in [
            ("/a/my-hook", "my-hook", ""),
            ("/a/my-hook/", "my-hook", "/"),
            ("/a/my-hook/404", "my-hook", "/404"),
            ("/a/my-hook/x/y", "my-hook", "/x/y"),
            ("/a/Not_An_Alias", "Not_An_Alias", ""),
        ] {
            let uri = uri(path);
            let routed = route(&uri).expect("routes");
            assert_eq!(routed.target, Target::Alias(name), "{path}");
            assert_eq!(routed.sub_path, sub_path, "{path}");
        }
    }

    #[test]
    fn paths_without_an_id_or_an_alias_name_do_not_route() {
        for path in [
            "/",
            "/a",
            "/a/",
            "/a//x",
            "/abc",
            "/abc/def",
            "/api/v1/instance",
        ] {
            assert_eq!(route(&uri(path)), None, "{path}");
        }
    }
}
