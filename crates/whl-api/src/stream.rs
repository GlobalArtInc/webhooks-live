use std::sync::Arc;

use whl_auth::{AccessControl, AccessMode, Decision, EndpointRole, decide_access};
use whl_core::endpoint_id::EndpointId;
use whl_live::{AllowAll, StreamAuthorizer, StreamRequest, StreamVerdict, VerdictFuture};

const STREAM_ROLE: EndpointRole = EndpointRole::Viewer;

#[derive(Debug, Clone)]
struct RoleAuthorizer {
    access: AccessControl,
}

impl StreamAuthorizer for RoleAuthorizer {
    fn still_allowed<'a>(
        &'a self,
        endpoint: &'a EndpointId,
        request: &'a StreamRequest,
    ) -> VerdictFuture<'a> {
        Box::pin(async move {
            let caller = match self.access.caller(request.headers(), request.peer()).await {
                Ok(caller) => caller,
                Err(rejection) => {
                    tracing::warn!(%rejection, "cannot resolve the caller of an open stream");
                    return StreamVerdict::Unavailable;
                }
            };
            match self.access.role_on(&caller, endpoint).await {
                Ok(role) => match decide_access(role, STREAM_ROLE) {
                    Decision::Allowed => StreamVerdict::Allowed,
                    Decision::Denied(_) => StreamVerdict::Denied,
                },
                Err(rejection) => {
                    tracing::warn!(%rejection, "cannot read the role of an open stream's viewer");
                    StreamVerdict::Unavailable
                }
            }
        })
    }
}

pub fn stream_authorizer(access: &AccessControl) -> Arc<dyn StreamAuthorizer> {
    match access.mode() {
        AccessMode::Open => Arc::new(AllowAll),
        AccessMode::Sso | AccessMode::Mixed => Arc::new(RoleAuthorizer {
            access: access.clone(),
        }),
    }
}
