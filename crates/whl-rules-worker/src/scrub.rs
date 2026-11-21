use whl_secrets::{RedactError, Redactor, Secret, SecretName};

pub(crate) struct Opened {
    secrets: Vec<(SecretName, Secret)>,
}

impl Opened {
    pub(crate) fn new(secrets: Vec<(SecretName, Secret)>) -> Self {
        Self { secrets }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Secret> {
        self.secrets
            .iter()
            .find(|(candidate, _)| candidate.unpack() == name)
            .map(|(_, secret)| secret)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SecretName, &Secret)> {
        self.secrets.iter().map(|(name, secret)| (name, secret))
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }
}

pub(crate) struct Scrubber {
    redactor: Option<Redactor>,
}

impl Scrubber {
    pub(crate) fn none() -> Self {
        Self { redactor: None }
    }

    pub(crate) fn new(opened: &Opened) -> Result<Self, RedactError> {
        if opened.is_empty() {
            return Ok(Self::none());
        }
        Ok(Self {
            redactor: Some(Redactor::new(opened.iter())?),
        })
    }

    pub(crate) fn text(&self, text: &str) -> String {
        match &self.redactor {
            Some(redactor) => redactor.redact_str(text).into_owned(),
            None => text.to_owned(),
        }
    }
}
