use crate::address::VerificationToken;
use crate::mail::{MAX_BODY_BYTES, OutgoingMail, ascii_line};
use crate::notice::Notice;

pub const SUBJECT_PREFIX: &str = "[webhooks.live]";
pub const VERIFICATION_HOURS: u64 = 24;

#[derive(Clone, PartialEq, Eq)]
pub struct ComposedMail {
    pub subject: String,
    pub body: String,
    pub unsubscribe: String,
}

impl std::fmt::Debug for ComposedMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComposedMail")
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

fn origin(public_url: &str) -> &str {
    public_url.trim_end_matches('/')
}

pub fn disable_endpoint_url(public_url: &str, token: &str) -> String {
    format!(
        "{}/api/v1/notifications/disable?token={token}",
        origin(public_url)
    )
}

pub fn disable_page_url(public_url: &str, token: &str) -> String {
    format!("{}/notifications/disable?token={token}", origin(public_url))
}

pub fn verification_url(public_url: &str, token: &VerificationToken) -> String {
    format!(
        "{}/email/verify?token={}",
        origin(public_url),
        token.unpack()
    )
}

pub fn compose_mail(notice: &Notice, public_url: &str, disable_token: &str) -> ComposedMail {
    let subject = ascii_line(
        &format!(
            "{SUBJECT_PREFIX} Notification for endpoint {}",
            notice.endpoint_id
        ),
        crate::mail::MAX_SUBJECT_CHARS,
    );
    let footer = format!(
        "\n\n--\nYou receive this because this address is verified for an email notification \
         channel on endpoint {}.\nTo stop these emails open: {}\n",
        notice.endpoint_id,
        disable_page_url(public_url, disable_token)
    );
    let room = MAX_BODY_BYTES.saturating_sub(footer.len());
    let mut text = notice.text.clone();
    if text.len() > room {
        let mut end = room;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    ComposedMail {
        subject,
        body: format!("{text}{footer}"),
        unsubscribe: disable_endpoint_url(public_url, disable_token),
    }
}

pub fn verification_mail(
    public_url: &str,
    address: &str,
    token: &VerificationToken,
) -> OutgoingMail {
    let link = verification_url(public_url, token);
    OutgoingMail {
        to: address.to_owned(),
        subject: format!("{SUBJECT_PREFIX} Confirm your email address"),
        body: format!(
            "Someone asked to receive webhooks.live notifications at this address.\n\n\
             To confirm it, open this link within {VERIFICATION_HOURS} hours:\n{link}\n\n\
             If you did not ask for this, ignore this message: nothing is sent to the address \
             until it is confirmed.\n"
        ),
        unsubscribe: None,
    }
}

#[cfg(test)]
mod tests {
    use whl_store::notify::DeliveryKind;

    use super::*;

    fn notice(text: &str) -> Notice {
        Notice {
            endpoint_id: "7k2m9q4xw8c1d5f3h6j0n2p4".to_owned(),
            text: text.to_owned(),
            delivery_kind: DeliveryKind::Single,
            capture: None,
        }
    }

    #[test]
    fn the_subject_never_carries_captured_content() {
        let mail = compose_mail(
            &notice("POST /Your account is locked"),
            "https://hooks.example.test/",
            "1.token",
        );
        assert_eq!(
            mail.subject,
            "[webhooks.live] Notification for endpoint 7k2m9q4xw8c1d5f3h6j0n2p4"
        );
        assert!(mail.body.contains("POST /Your account is locked"));
        assert!(
            mail.unsubscribe
                .starts_with("https://hooks.example.test/api/v1/notifications/disable?token=")
        );
    }

    #[test]
    fn an_oversized_text_is_cut_so_the_footer_and_the_limit_survive() {
        let mail = compose_mail(
            &notice(&"\u{e9}".repeat(MAX_BODY_BYTES)),
            "https://hooks.example.test",
            "1.token",
        );
        assert!(mail.body.len() <= MAX_BODY_BYTES);
        assert!(mail.body.contains("To stop these emails open: "));
    }
}
