#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Verification {
    Valid,
    Invalid,
    Missing,
}

impl Verification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Missing => "missing",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "valid" => Some(Self::Valid),
            "invalid" => Some(Self::Invalid),
            "missing" => Some(Self::Missing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    Matched,
    SignatureAbsent,
    SignatureMalformed,
    TimestampAbsent,
    TimestampMalformed,
    TimestampOutOfTolerance,
    MessageIdAbsent,
    MessageIdMalformed,
    Mismatch,
    SecretUnusable,
    Internal,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "matched",
            Self::SignatureAbsent => "signature_absent",
            Self::SignatureMalformed => "signature_malformed",
            Self::TimestampAbsent => "timestamp_absent",
            Self::TimestampMalformed => "timestamp_malformed",
            Self::TimestampOutOfTolerance => "timestamp_out_of_tolerance",
            Self::MessageIdAbsent => "message_id_absent",
            Self::MessageIdMalformed => "message_id_malformed",
            Self::Mismatch => "mismatch",
            Self::SecretUnusable => "secret_unusable",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub verification: Verification,
    pub reason: Reason,
}

impl Verdict {
    pub(crate) fn valid() -> Self {
        Self {
            verification: Verification::Valid,
            reason: Reason::Matched,
        }
    }

    pub(crate) fn invalid(reason: Reason) -> Self {
        Self {
            verification: Verification::Invalid,
            reason,
        }
    }

    pub fn missing() -> Self {
        Self {
            verification: Verification::Missing,
            reason: Reason::SignatureAbsent,
        }
    }

    pub fn unusable_secret() -> Self {
        Self::invalid(Reason::SecretUnusable)
    }

    pub fn internal() -> Self {
        Self::invalid(Reason::Internal)
    }
}
