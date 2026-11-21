use std::sync::Arc;

use whl_auth::{
    AccessControl, AccessMode, AuthRejection, Decision, EndpointRole, TokenScope, decide_access,
};
use whl_core::endpoint_id::EndpointId;
use whl_live::{StreamAuthorizer, StreamRequest, StreamVerdict, VerdictFuture};

const STREAM_ROLE: EndpointRole = EndpointRole::Viewer;

#[derive(Debug, Clone)]
struct RoleAuthorizer {
    access: AccessControl,
}

impl RoleAuthorizer {
    async fn role_verdict(&self, endpoint: &EndpointId, request: &StreamRequest) -> StreamVerdict {
        let caller = match self.access.caller(request.headers(), request.peer()).await {
            Ok(caller) => caller,
            Err(AuthRejection::InvalidToken) => return StreamVerdict::Denied,
            Err(rejection) => {
                tracing::warn!(%rejection, "cannot resolve the caller of an open stream");
                return StreamVerdict::Unavailable;
            }
        };
        if !self.access.token_covers(&caller, TokenScope::Read) {
            return StreamVerdict::Denied;
        }
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
    }

    async fn grant_verdict(&self, endpoint: &EndpointId, request: &StreamRequest) -> StreamVerdict {
        match self
            .access
            .view_grant_valid(endpoint, request.headers(), request.query())
            .await
        {
            Ok(true) => StreamVerdict::Allowed,
            Ok(false) => StreamVerdict::Denied,
            Err(rejection) => {
                tracing::warn!(%rejection, "cannot read the view grant of an open stream");
                StreamVerdict::Unavailable
            }
        }
    }
}

impl StreamAuthorizer for RoleAuthorizer {
    fn still_allowed<'a>(
        &'a self,
        endpoint: &'a EndpointId,
        request: &'a StreamRequest,
    ) -> VerdictFuture<'a> {
        Box::pin(async move {
            if self.access.mode() != AccessMode::Open {
                let verdict = self.role_verdict(endpoint, request).await;
                if verdict != StreamVerdict::Allowed {
                    return verdict;
                }
            }
            self.grant_verdict(endpoint, request).await
        })
    }
}

pub fn stream_authorizer(access: &AccessControl) -> Arc<dyn StreamAuthorizer> {
    Arc::new(RoleAuthorizer {
        access: access.clone(),
    })
}
