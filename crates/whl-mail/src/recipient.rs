use whl_core::endpoint_id::EndpointId;

use crate::settings::MailDomain;

pub const MAX_TAG_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub endpoint: EndpointId,
    pub tag: Option<String>,
    pub address: String,
}

fn tag_is_valid(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= MAX_TAG_BYTES
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

pub fn parse_recipient(address: &str, domain: &MailDomain) -> Option<Recipient> {
    let (local, host) = address.rsplit_once('@')?;
    if !domain.matches(host) || !local.is_ascii() {
        return None;
    }
    let local = local.to_ascii_lowercase();
    let (id_part, tag) = match local.split_once('+') {
        Some((id, tag)) => (id, Some(tag)),
        None => (local.as_str(), None),
    };
    if tag.is_some_and(|tag| !tag_is_valid(tag)) {
        return None;
    }
    let endpoint = EndpointId::parse(id_part).ok()?;
    Some(Recipient {
        address: format!("{local}@{}", domain.unpack()),
        tag: tag.map(str::to_owned),
        endpoint,
    })
}
