use std::time::Duration;

use whl_core::endpoint_id::EndpointId;
use whl_rules::{StateBackend, StateError, StoreCall, StoreValue};
use whl_store::Store;
use whl_store::vars::{IncrementOutcome, PutVarOutcome};

pub struct StoreState<'a> {
    pub store: &'a Store,
    pub endpoint: &'a EndpointId,
    pub retention: Duration,
}

impl StateBackend for StoreState<'_> {
    async fn write(&self, call: StoreCall) -> Result<StoreValue, StateError> {
        match call {
            StoreCall::Set { name, value } => {
                match self
                    .store
                    .put_var(self.endpoint, self.retention, &name, &value)
                    .await
                {
                    Ok(PutVarOutcome::Stored(_)) => Ok(StoreValue::Written),
                    Ok(PutVarOutcome::EndpointGone) => Err(StateError::EndpointGone),
                    Ok(PutVarOutcome::LimitReached) => Err(StateError::LimitReached),
                    Ok(PutVarOutcome::Rejected) => Err(StateError::Rejected),
                    Err(error) => {
                        tracing::warn!(%error, endpoint_id = self.endpoint.unpack(), "a rule could not store a variable");
                        Err(StateError::Unavailable)
                    }
                }
            }
            StoreCall::Add { name, by } => {
                match self
                    .store
                    .add_to_counter(self.endpoint, self.retention, &name, by)
                    .await
                {
                    Ok(IncrementOutcome::Value(value)) => Ok(StoreValue::Counter(value)),
                    Ok(IncrementOutcome::EndpointGone) => Err(StateError::EndpointGone),
                    Ok(IncrementOutcome::LimitReached) => Err(StateError::LimitReached),
                    Ok(IncrementOutcome::NotACounter) => Err(StateError::NotACounter),
                    Err(error) => {
                        tracing::warn!(%error, endpoint_id = self.endpoint.unpack(), "a rule could not change a counter");
                        Err(StateError::Unavailable)
                    }
                }
            }
        }
    }
}
