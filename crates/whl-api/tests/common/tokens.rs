use axum::http::{Method, StatusCode};
use serde_json::json;

use super::{Actor, Reply, SsoHarness};

pub const ALL_SCOPES: [&str; 5] = [
    "read",
    "endpoints:write",
    "captures:write",
    "rules:write",
    "send",
];

pub struct Minted {
    pub id: String,
    pub token: String,
}

pub async fn mint(
    harness: &SsoHarness,
    actor: &Actor,
    scopes: &[&str],
    endpoints: Option<&[&str]>,
) -> Minted {
    let body = json!({
        "name": "test token",
        "scopes": scopes,
        "endpoint_ids": endpoints,
        "expires_in_days": 30,
    });
    let reply = harness
        .send_as(
            Some(actor),
            Method::POST,
            "/api/v1/tokens",
            &body.to_string(),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CREATED, "{:?}", reply.json());
    let json = reply.json();
    Minted {
        id: json["id"].as_str().expect("an id").to_owned(),
        token: json["token"].as_str().expect("a token").to_owned(),
    }
}

impl SsoHarness {
    pub async fn send_bearer(&self, token: &str, method: Method, uri: &str, body: &str) -> Reply {
        self.send_with(
            None,
            None,
            &[("authorization", &format!("Bearer {token}"))],
            method,
            uri,
            body,
        )
        .await
    }
}
