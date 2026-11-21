use whl_secrets::{NameError, SecretName};
use whl_store::Uuid;

pub fn channel_secret_name(channel: Uuid) -> Result<SecretName, NameError> {
    SecretName::parse(&format!(
        "C{}",
        channel.simple().to_string().to_ascii_uppercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_a_valid_secret_name_bound_to_the_channel() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let name = channel_secret_name(first).expect("a valid name");
        assert_eq!(name.unpack().len(), 33);
        assert_ne!(name, channel_secret_name(second).expect("a valid name"));
    }
}
