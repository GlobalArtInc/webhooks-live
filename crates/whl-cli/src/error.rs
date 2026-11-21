use std::io;

use crate::api::ApiError;
use crate::state::StateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Failure,
    Usage,
    Auth,
    EndpointGone,
    State,
}

impl ExitStatus {
    pub fn code(self) -> u8 {
        match self {
            Self::Failure => 1,
            Self::Usage => 2,
            Self::Auth => 3,
            Self::EndpointGone => 4,
            Self::State => 5,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error("cannot write output: {0}")]
    Output(#[from] io::Error),
    #[error("{0}")]
    TokenRefused(String),
    #[error("{0}")]
    Runtime(String),
}

impl CliError {
    pub fn status(&self) -> ExitStatus {
        match self {
            Self::Usage(_) => ExitStatus::Usage,
            Self::Api(ApiError::Unauthorized { .. }) | Self::TokenRefused(_) => ExitStatus::Auth,
            Self::Api(ApiError::Gone { .. }) => ExitStatus::EndpointGone,
            Self::State(_) => ExitStatus::State,
            Self::Api(_) | Self::Output(_) | Self::Runtime(_) => ExitStatus::Failure,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_failure_class_to_its_own_exit_code() {
        let auth = CliError::Api(ApiError::Unauthorized {
            status: 401,
            message: "invalid_token".to_owned(),
        });
        let gone = CliError::Api(ApiError::Gone {
            message: "endpoint_gone".to_owned(),
        });
        assert_eq!(auth.status().code(), 3);
        assert_eq!(gone.status().code(), 4);
        assert_eq!(CliError::Usage("x".to_owned()).status().code(), 2);
        assert_eq!(CliError::Runtime("x".to_owned()).status().code(), 1);
    }
}
