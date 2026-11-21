use std::collections::BTreeSet;

use crate::limits::ParseLimits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTarget {
    Body,
    HeaderValue,
    Status,
    JsonString,
}

impl RenderTarget {
    pub fn default_output_limit(self) -> usize {
        match self {
            Self::Body | Self::JsonString => 1024 * 1024,
            Self::HeaderValue => 1024,
            Self::Status => 3,
        }
    }

    pub(crate) fn accepts(self, character: char) -> bool {
        match self {
            Self::Body | Self::JsonString => true,
            Self::HeaderValue => character == '\t' || (' '..='~').contains(&character),
            Self::Status => character.is_ascii_digit(),
        }
    }

    pub(crate) fn first_forbidden(self, text: &str) -> Option<(usize, char)> {
        text.char_indices()
            .find(|(_, character)| !self.accepts(*character))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretPolicy {
    Allowed,
    Forbidden,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepPolicy {
    Any,
    Only(BTreeSet<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub target: RenderTarget,
    pub secrets: SecretPolicy,
    pub steps: StepPolicy,
    pub limits: ParseLimits,
}

impl Policy {
    pub fn new(target: RenderTarget, secrets: SecretPolicy) -> Self {
        Self {
            target,
            secrets,
            steps: StepPolicy::Any,
            limits: ParseLimits::default(),
        }
    }

    pub fn response(target: RenderTarget) -> Self {
        Self::new(target, SecretPolicy::Forbidden)
    }

    pub fn outbound(target: RenderTarget) -> Self {
        Self::new(target, SecretPolicy::Allowed)
    }
}
