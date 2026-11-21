use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use whl_store::Order;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub order: Order,
    pub position: i64,
    pub fingerprint: String,
}

impl Cursor {
    pub fn encode(&self) -> String {
        let text = format!(
            "{}|{}|{}",
            self.order.as_str(),
            self.position,
            self.fingerprint
        );
        URL_SAFE_NO_PAD.encode(text)
    }

    pub fn decode(text: &str) -> Option<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
        let decoded = String::from_utf8(bytes).ok()?;
        let mut parts = decoded.splitn(3, '|');
        let order = match parts.next()? {
            "asc" => Order::Asc,
            "desc" => Order::Desc,
            _ => return None,
        };
        let position = parts.next()?.parse().ok()?;
        let fingerprint = parts.next()?.to_owned();
        Some(Self {
            order,
            position,
            fingerprint,
        })
    }
}
