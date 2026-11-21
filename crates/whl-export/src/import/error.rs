use std::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Document,
    Capture(usize),
    Line(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    Required,
    InvalidFormat,
    TooLong,
    OutOfRange,
    TooMany,
    NotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportViolation {
    pub place: Place,
    pub path: String,
    pub problem: Problem,
    pub text: String,
}

impl ImportViolation {
    pub fn new(place: Place, path: impl Into<String>, problem: Problem, text: &str) -> Self {
        Self {
            place,
            path: path.into(),
            problem,
            text: text.to_owned(),
        }
    }

    pub fn field(&self) -> String {
        match self.place {
            Place::Document => self.path.clone(),
            Place::Capture(index) if self.path.is_empty() => format!("captures.{index}"),
            Place::Capture(index) => format!("captures.{index}.{}", self.path),
            Place::Line(number) => format!("line.{number}"),
        }
    }

    pub fn message(&self) -> String {
        match self.place {
            Place::Line(_) if !self.path.is_empty() => format!("{} {}", self.path, self.text),
            _ => self.text.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("the import is larger than {max} bytes")]
    TooLarge { max: u64 },
    #[error("the import cannot be read: {0}")]
    Read(#[source] Box<dyn Error + Send + Sync>),
    #[error("the import is invalid at {}: {}", .0.field(), .0.message())]
    Invalid(ImportViolation),
}

impl ImportError {
    pub(crate) fn invalid(place: Place, path: &str, problem: Problem, text: &str) -> Self {
        Self::Invalid(ImportViolation::new(place, path, problem, text))
    }
}
