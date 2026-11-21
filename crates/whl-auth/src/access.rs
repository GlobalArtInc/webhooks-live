use whl_store::EndpointRole;

use crate::error::AccessDenied;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied(AccessDenied),
}

impl Decision {
    pub fn into_result(self) -> Result<(), AccessDenied> {
        match self {
            Self::Allowed => Ok(()),
            Self::Denied(denied) => Err(denied),
        }
    }
}

pub fn decide_access(role: Option<EndpointRole>, needed: EndpointRole) -> Decision {
    match role {
        None => Decision::Denied(AccessDenied::Gone),
        Some(held) if held >= needed => Decision::Allowed,
        Some(_) => Decision::Denied(AccessDenied::Forbidden),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    const ROLES: [EndpointRole; 3] = [
        EndpointRole::Viewer,
        EndpointRole::Editor,
        EndpointRole::Owner,
    ];

    #[test]
    fn no_role_is_gone_for_every_requirement() {
        for needed in ROLES {
            assert_eq!(
                decide_access(None, needed),
                Decision::Denied(AccessDenied::Gone)
            );
        }
    }

    #[test]
    fn role_order_is_viewer_then_editor_then_owner() {
        for (held_index, held) in ROLES.into_iter().enumerate() {
            for (needed_index, needed) in ROLES.into_iter().enumerate() {
                let expected = if held_index >= needed_index {
                    Decision::Allowed
                } else {
                    Decision::Denied(AccessDenied::Forbidden)
                };
                assert_eq!(decide_access(Some(held), needed), expected);
            }
        }
    }

    #[test]
    fn into_result_carries_the_denial() {
        assert_eq!(Decision::Allowed.into_result(), Ok(()));
        assert_eq!(
            decide_access(None, EndpointRole::Viewer).into_result(),
            Err(AccessDenied::Gone)
        );
    }

    async fn code_of(denied: AccessDenied) -> (StatusCode, String) {
        let response = denied.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .expect("the body is readable");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("the body is JSON");
        (
            status,
            body["error"]["code"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        )
    }

    #[tokio::test]
    async fn denials_map_to_gone_and_forbidden_bodies() {
        assert_eq!(
            code_of(AccessDenied::Gone).await,
            (StatusCode::GONE, "endpoint_gone".to_owned())
        );
        assert_eq!(
            code_of(AccessDenied::Forbidden).await,
            (StatusCode::FORBIDDEN, "forbidden".to_owned())
        );
    }
}
