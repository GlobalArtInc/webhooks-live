pub const MAX_TAGS: usize = 10;
const MAX_TAG_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("must be 1 to 32 characters of a to z, 0 to 9 and hyphen, not starting with a hyphen")]
pub struct TagError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag(String);

impl Tag {
    pub fn parse(text: &str) -> Result<Self, TagError> {
        let bytes = text.as_bytes();
        let valid = (1..=MAX_TAG_BYTES).contains(&bytes.len())
            && bytes.iter().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || (index > 0 && *byte == b'-')
            });
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(TagError)
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_takes_lowercase_letters_digits_and_inner_hyphens() {
        assert_eq!(Tag::parse("prod-eu2").expect("valid").unpack(), "prod-eu2");
        assert!(Tag::parse("Prod").is_err());
        assert!(Tag::parse("-prod").is_err());
    }

    #[test]
    fn a_tag_is_at_most_32_characters() {
        assert!(Tag::parse(&"a".repeat(32)).is_ok());
        assert!(Tag::parse(&"a".repeat(33)).is_err());
        assert!(Tag::parse("").is_err());
    }
}
