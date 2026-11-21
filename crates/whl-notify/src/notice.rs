use serde_json::{Value, json};
use whl_store::notify::{ClaimedDelivery, DeliveryKind, NotifyCapture};
use whl_template::{
    BodyData, CaptureData, ChaChaRandom, Context, EndpointData, NameValue, Policy, RandomSeed,
    RenderEnv, RenderLimits, RenderTarget, RequestData, SignatureState, SystemClock, Template,
};

pub const MAX_NOTICE_CHARS: usize = 8000;
pub const MAX_RENDERED_BYTES: usize = 16 * 1024;
pub const MAX_PATH_CHARS: usize = 1024;
pub const MAX_LABEL_CHARS: usize = 80;
pub const MAX_PREVIEW_PATH_CHARS: usize = 200;

const ELLIPSIS: char = '\u{2026}';

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub endpoint_id: String,
    pub text: String,
    pub capture: Option<Value>,
}

pub struct ComposeInput<'a> {
    pub delivery: &'a ClaimedDelivery,
    pub capture: Option<&'a NotifyCapture>,
    pub public_url: &'a str,
}

pub fn compose(input: &ComposeInput<'_>) -> Notice {
    let delivery = input.delivery;
    let endpoint_id = delivery.channel.endpoint_id.clone();
    let text = match (delivery.kind, input.capture) {
        (DeliveryKind::Single, Some(capture)) => single_text(input, capture),
        (DeliveryKind::Summary, _) => summary_text(delivery),
        _ => delivery
            .message
            .clone()
            .unwrap_or_else(|| "Test notification from webhooks.live.".to_owned()),
    };
    Notice {
        endpoint_id,
        text: truncate(&clean(&text), MAX_NOTICE_CHARS),
        capture: input
            .capture
            .map(|capture| capture_summary(capture, &delivery.channel.endpoint_id)),
    }
}

pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    kept.push(ELLIPSIS);
    kept
}

fn endpoint_label(delivery: &ClaimedDelivery) -> String {
    let label = delivery
        .channel
        .endpoint_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(&delivery.channel.endpoint_id);
    truncate(&clean(label), MAX_LABEL_CHARS)
}

fn capture_link(public_url: &str, endpoint: &str, capture: &NotifyCapture) -> String {
    format!(
        "{}/e/{endpoint}/c/{}",
        public_url.trim_end_matches('/'),
        capture.id.hyphenated()
    )
}

fn default_text(input: &ComposeInput<'_>, capture: &NotifyCapture) -> String {
    let label = endpoint_label(input.delivery);
    let path = truncate(&clean(&capture.path), MAX_PREVIEW_PATH_CHARS);
    let link = capture_link(
        input.public_url,
        &input.delivery.channel.endpoint_id,
        capture,
    );
    format!(
        "New {} request on endpoint {label}: {} {path}\n{link}",
        capture.kind.as_str(),
        clean(&capture.method)
    )
}

fn single_text(input: &ComposeInput<'_>, capture: &NotifyCapture) -> String {
    if let Some(message) = input
        .delivery
        .message
        .as_deref()
        .filter(|message| !message.trim().is_empty())
    {
        return message.to_owned();
    }
    let Some(source) = input.delivery.channel.template.as_deref() else {
        return default_text(input, capture);
    };
    render_template(source, input, capture).unwrap_or_else(|| default_text(input, capture))
}

fn render_template(
    source: &str,
    input: &ComposeInput<'_>,
    capture: &NotifyCapture,
) -> Option<String> {
    let policy = Policy::response(RenderTarget::Body);
    let template = Template::parse(source, &policy).ok()?;
    let context = context_of(input, capture);
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.max_output_bytes = MAX_RENDERED_BYTES;
    let mut random = ChaChaRandom::from_seed(&RandomSeed::from_bytes(input.delivery.id.as_bytes()));
    let clock = SystemClock::new();
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(&context, &limits, &mut env);
    let output = outcome.output().to_owned();
    (!output.trim().is_empty()).then_some(output)
}

fn pairs(items: &[whl_store::captures::NameValue]) -> Vec<NameValue> {
    items
        .iter()
        .map(|item| NameValue::new(item.name.clone(), item.value.clone()))
        .collect()
}

fn context_of(input: &ComposeInput<'_>, capture: &NotifyCapture) -> Context {
    let mut context = Context::new(time::OffsetDateTime::now_utc());
    context.request = RequestData {
        method: capture.method.clone(),
        path: capture.path.clone(),
        segments: capture
            .path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect(),
        query: pairs(&capture.query),
        headers: pairs(&capture.headers),
        form: Vec::new(),
        body: BodyData::new(capture.body.to_vec()),
        ip: capture.client_ip.clone(),
        content_type: capture.content_type.clone().unwrap_or_default(),
        kind: capture.kind.as_str().to_owned(),
        meta: Vec::new(),
        signature: SignatureState::Missing,
    };
    context.capture = CaptureData {
        id: capture.id.hyphenated().to_string(),
        seq: capture.seq,
        received_at: capture.received_at,
    };
    let endpoint = &input.delivery.channel.endpoint_id;
    context.endpoint = EndpointData {
        id: endpoint.clone(),
        url: format!("{}/{endpoint}", input.public_url.trim_end_matches('/')),
        title: input
            .delivery
            .channel
            .endpoint_title
            .clone()
            .unwrap_or_default(),
    };
    context
}

fn summary_text(delivery: &ClaimedDelivery) -> String {
    let count = delivery.suppressed_count.unwrap_or(0);
    let window = delivery
        .window_start
        .as_deref()
        .unwrap_or("an earlier hour");
    format!(
        "{count} notifications on endpoint {} were suppressed in the hour starting {window} \
         because a rate limit was reached.",
        endpoint_label(delivery)
    )
}

fn capture_summary(capture: &NotifyCapture, endpoint: &str) -> Value {
    json!({
        "id": capture.id.hyphenated().to_string(),
        "seq": capture.seq,
        "endpoint_id": endpoint,
        "received_at": capture.received_text,
        "method": capture.method,
        "path": truncate(&capture.path, MAX_PATH_CHARS),
        "query_string": truncate(&capture.query_string, MAX_PATH_CHARS),
        "client_ip": capture.client_ip,
        "content_type": capture.content_type,
        "body_size": capture.body_size,
        "status_override": capture.status_override,
        "response_status": capture.response_status,
        "forward_state": Value::Null,
        "kind": capture.kind.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use time::OffsetDateTime;
    use whl_store::notify::{ChannelKind, DeliveryChannel};
    use whl_store::{CaptureKind, Uuid};

    use super::*;

    fn delivery(message: Option<&str>) -> ClaimedDelivery {
        ClaimedDelivery {
            id: Uuid::nil(),
            channel_id: Uuid::nil(),
            capture_id: Some(Uuid::nil()),
            kind: DeliveryKind::Single,
            message: message.map(str::to_owned),
            attempt: 1,
            quota_checked: false,
            window_start: None,
            suppressed_count: None,
            channel: DeliveryChannel {
                kind: ChannelKind::Webhook,
                enabled: true,
                config: json!({}),
                sealed: None,
                template: None,
                per_hour: 30,
                endpoint_id: "ep".to_owned(),
                endpoint_title: None,
                owner_id: None,
                creator_net: None,
            },
        }
    }

    fn capture() -> NotifyCapture {
        NotifyCapture {
            id: Uuid::nil(),
            seq: 1,
            received_at: OffsetDateTime::UNIX_EPOCH,
            received_text: "1970-01-01T00:00:00.000Z".to_owned(),
            method: "POST".to_owned(),
            url: "http://h/x".to_owned(),
            path: "/x".to_owned(),
            query_string: String::new(),
            host: "h".to_owned(),
            client_ip: "203.0.113.1".to_owned(),
            content_type: None,
            headers: Vec::new(),
            query: Vec::new(),
            body_size: 0,
            status_override: None,
            response_status: 200,
            kind: CaptureKind::Http,
            body: Bytes::new(),
        }
    }

    fn text_of(delivery: &ClaimedDelivery) -> String {
        let capture = capture();
        compose(&ComposeInput {
            delivery,
            capture: Some(&capture),
            public_url: "https://whl.test",
        })
        .text
    }

    #[test]
    fn a_message_written_by_a_rule_replaces_the_default_text_of_a_single_notification() {
        assert_eq!(
            text_of(&delivery(Some("Order 42 arrived"))),
            "Order 42 arrived"
        );
        assert!(text_of(&delivery(None)).starts_with("New http request on endpoint"));
    }
}
