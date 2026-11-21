use std::borrow::Cow;

use axum::body::Bytes;
use axum::http::HeaderValue;
use rust_embed::RustEmbed;

pub(crate) trait AssetSource: Send + Sync {
    fn get(&self, path: &str) -> Option<Asset>;
}

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
pub(crate) struct Dist;

impl AssetSource for Dist {
    fn get(&self, path: &str) -> Option<Asset> {
        <Self as RustEmbed>::get(path)
            .map(|file| Asset::new(file.data, file.metadata.sha256_hash()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ETag(HeaderValue);

impl ETag {
    pub(crate) fn from_digest(digest: &[u8; 32]) -> Self {
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        Self(
            HeaderValue::from_str(&format!("\"{hex}\""))
                .unwrap_or_else(|_| HeaderValue::from_static("\"\"")),
        )
    }

    pub(crate) fn unpack(&self) -> &HeaderValue {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Asset {
    pub(crate) body: Bytes,
    pub(crate) etag: ETag,
}

impl Asset {
    pub(crate) fn new(body: Cow<'static, [u8]>, digest: [u8; 32]) -> Self {
        let body = match body {
            Cow::Borrowed(bytes) => Bytes::from_static(bytes),
            Cow::Owned(bytes) => Bytes::from(bytes),
        };
        Self {
            body,
            etag: ETag::from_digest(&digest),
        }
    }
}
