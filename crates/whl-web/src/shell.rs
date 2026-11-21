use std::borrow::Cow;

use axum::http::HeaderValue;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

use crate::source::{Asset, AssetSource};

const PLACEHOLDER: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"UTF-8\">\n<title>webhooks.live</title>\n</head>\n<body>\n<p>The front end is not built. Run the web build so that web/dist exists, then rebuild the server.</p>\n</body>\n</html>\n";

const SCRIPT_OPEN: &str = "<script";
const SCRIPT_CLOSE: &str = "</script>";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ShellError {
    #[error("index.html is not valid UTF-8")]
    NotUtf8,
    #[error("script tag at byte {0} has no closing bracket")]
    UnterminatedTag(usize),
    #[error("inline script at byte {0} has no closing tag")]
    UnterminatedScript(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptHash(String);

impl ScriptHash {
    pub(crate) fn unpack(&self) -> &str {
        &self.0
    }
}

pub(crate) struct Shell {
    pub(crate) document: Asset,
    pub(crate) csp: HeaderValue,
}

impl Shell {
    pub(crate) fn load(source: &dyn AssetSource) -> Self {
        let document = source.get("index.html").unwrap_or_else(placeholder);
        let hashes = inline_script_hashes(&document.body).unwrap_or_default();
        Self {
            csp: policy(&hashes),
            document,
        }
    }
}

fn placeholder() -> Asset {
    Asset::new(
        Cow::Borrowed(PLACEHOLDER.as_bytes()),
        Sha256::digest(PLACEHOLDER).into(),
    )
}

pub(crate) fn inline_script_hashes(html: &[u8]) -> Result<Vec<ScriptHash>, ShellError> {
    let text = std::str::from_utf8(html).map_err(|_| ShellError::NotUtf8)?;
    let lower = text.to_ascii_lowercase();
    let mut hashes = Vec::new();
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find(SCRIPT_OPEN) {
        let open = cursor + found;
        let name_end = open + SCRIPT_OPEN.len();
        if !matches!(lower[name_end..].chars().next(), Some(c) if c.is_ascii_whitespace() || c == '>' || c == '/')
        {
            cursor = name_end;
            continue;
        }
        let tag_end = lower[name_end..]
            .find('>')
            .map(|offset| name_end + offset)
            .ok_or(ShellError::UnterminatedTag(open))?;
        let body_start = tag_end + 1;
        let body_end = lower[body_start..]
            .find(SCRIPT_CLOSE)
            .map(|offset| body_start + offset)
            .ok_or(ShellError::UnterminatedScript(open))?;
        if !has_src(&lower[name_end..tag_end]) {
            let digest = Sha256::digest(&text.as_bytes()[body_start..body_end]);
            hashes.push(ScriptHash(format!("'sha256-{}'", STANDARD.encode(digest))));
        }
        cursor = body_end + SCRIPT_CLOSE.len();
    }
    Ok(hashes)
}

fn has_src(attributes: &str) -> bool {
    attributes
        .split_ascii_whitespace()
        .any(|attribute| attribute == "src" || attribute.starts_with("src="))
}

pub(crate) fn policy(script_hashes: &[ScriptHash]) -> HeaderValue {
    let mut script_src = String::from("'self'");
    for hash in script_hashes {
        script_src.push(' ');
        script_src.push_str(hash.unpack());
    }
    let value = format!(
        "default-src 'none'; script-src {script_src}; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob:; worker-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"
    );
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("default-src 'none'"))
}
