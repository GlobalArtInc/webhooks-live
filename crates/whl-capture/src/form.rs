use std::time::Duration;

use bytes::Bytes;
use futures_util::stream;
use multer::Multipart;
use url::form_urlencoded;
use whl_store::captures::{Form, FormFile, NameValue};

const SLICE_BYTES: usize = 64 * 1024;
const URLENCODED: &str = "application/x-www-form-urlencoded";
const MULTIPART: &str = "multipart/form-data";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormLimits {
    pub max_query_pairs: usize,
    pub max_fields: usize,
    pub max_text_bytes: usize,
    pub max_text_field_bytes: usize,
    pub parse_deadline: Duration,
}

impl Default for FormLimits {
    fn default() -> Self {
        Self {
            max_query_pairs: 1000,
            max_fields: 1000,
            max_text_bytes: 512 * 1024,
            max_text_field_bytes: 64 * 1024,
            parse_deadline: Duration::from_secs(2),
        }
    }
}

pub fn parse_query(query: Option<&str>, limits: &FormLimits) -> Vec<NameValue> {
    let Some(query) = query else {
        return Vec::new();
    };
    form_urlencoded::parse(query.as_bytes())
        .take(limits.max_query_pairs)
        .map(|(name, value)| NameValue {
            name: name.into_owned(),
            value: value.into_owned(),
        })
        .collect()
}

pub async fn parse_form(
    content_type: Option<&str>,
    body: &Bytes,
    limits: &FormLimits,
) -> Option<Form> {
    let content_type = content_type?;
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match essence.as_str() {
        URLENCODED => parse_urlencoded(body, limits),
        MULTIPART => {
            let boundary = multer::parse_boundary(content_type).ok()?;
            tokio::time::timeout(
                limits.parse_deadline,
                parse_multipart(boundary, body.clone(), limits),
            )
            .await
            .ok()
            .flatten()
        }
        _ => None,
    }
}

fn parse_urlencoded(body: &[u8], limits: &FormLimits) -> Option<Form> {
    let mut form = Form::default();
    let mut text_bytes = 0_usize;
    for (name, value) in form_urlencoded::parse(body) {
        if form.fields.len() >= limits.max_fields {
            return None;
        }
        text_bytes += name.len() + value.len();
        if text_bytes > limits.max_text_bytes {
            return None;
        }
        form.fields.push(NameValue {
            name: name.into_owned(),
            value: value.into_owned(),
        });
    }
    Some(form)
}

fn slices(
    body: Bytes,
) -> impl futures_util::Stream<Item = Result<Bytes, std::convert::Infallible>> {
    stream::unfold((body, 0_usize), |(body, offset)| async move {
        if offset >= body.len() {
            return None;
        }
        let end = (offset + SLICE_BYTES).min(body.len());
        let slice = body.slice(offset..end);
        if offset > 0 {
            tokio::task::yield_now().await;
        }
        Some((Ok(slice), (body, end)))
    })
}

async fn parse_multipart(boundary: String, body: Bytes, limits: &FormLimits) -> Option<Form> {
    let mut multipart = Multipart::new(slices(body), boundary);
    let mut form = Form::default();
    let mut parts = 0_usize;
    let mut text_bytes = 0_usize;
    while let Some(mut field) = multipart.next_field().await.ok()? {
        parts += 1;
        if parts > limits.max_fields {
            return None;
        }
        let name = field.name().unwrap_or_default().to_owned();
        let filename = field.file_name().map(str::to_owned);
        let content_type = field.content_type().map(ToString::to_string);
        text_bytes += name.len()
            + filename.as_ref().map_or(0, String::len)
            + content_type.as_ref().map_or(0, String::len);
        if text_bytes > limits.max_text_bytes {
            return None;
        }

        if filename.is_some() {
            let mut size = 0_u64;
            while let Some(chunk) = field.chunk().await.ok()? {
                size += chunk.len() as u64;
            }
            form.files.push(FormFile {
                name,
                filename,
                content_type,
                size,
            });
            continue;
        }

        let mut value = Vec::new();
        while let Some(chunk) = field.chunk().await.ok()? {
            let room = limits.max_text_field_bytes.saturating_sub(value.len());
            value.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
        text_bytes += value.len();
        if text_bytes > limits.max_text_bytes {
            return None;
        }
        form.fields.push(NameValue {
            name,
            value: String::from_utf8_lossy(&value).into_owned(),
        });
    }
    Some(form)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> FormLimits {
        FormLimits::default()
    }

    #[tokio::test]
    async fn urlencoded_fields_keep_order_and_decode() {
        let body = Bytes::from_static(b"a=1&b=hello+world&a=%E2%9C%93");
        let form = parse_form(Some("application/x-www-form-urlencoded"), &body, &limits())
            .await
            .expect("form parses");
        let pairs: Vec<_> = form
            .fields
            .iter()
            .map(|pair| (pair.name.as_str(), pair.value.as_str()))
            .collect();
        assert_eq!(pairs, [("a", "1"), ("b", "hello world"), ("a", "\u{2713}")]);
        assert!(form.files.is_empty());
    }

    #[tokio::test]
    async fn urlencoded_over_the_field_cap_is_not_parsed() {
        let body = Bytes::from("a=1&".repeat(2000));
        assert!(
            parse_form(Some("application/x-www-form-urlencoded"), &body, &limits())
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn multipart_splits_text_fields_and_file_descriptors() {
        let body = Bytes::from_static(
            b"--XX\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nhello\r\n\
              --XX\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"a.bin\"\r\n\
              Content-Type: application/octet-stream\r\n\r\n0123456789\r\n--XX--\r\n",
        );
        let form = parse_form(Some("multipart/form-data; boundary=XX"), &body, &limits())
            .await
            .expect("form parses");
        assert_eq!(form.fields.len(), 1);
        assert_eq!(form.fields[0].value, "hello");
        assert_eq!(
            form.files,
            [FormFile {
                name: "upload".into(),
                filename: Some("a.bin".into()),
                content_type: Some("application/octet-stream".into()),
                size: 10,
            }]
        );
    }

    #[tokio::test]
    async fn multipart_with_too_many_parts_is_abandoned() {
        let mut raw = Vec::new();
        for _ in 0..50_000 {
            raw.extend_from_slice(
                b"--XX\r\nContent-Disposition: form-data; name=\"f\"\r\n\r\n\r\n",
            );
        }
        raw.extend_from_slice(b"--XX--\r\n");
        let started = std::time::Instant::now();
        let form = parse_form(
            Some("multipart/form-data; boundary=XX"),
            &Bytes::from(raw),
            &limits(),
        )
        .await;
        assert!(form.is_none());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn other_content_types_have_no_form() {
        let body = Bytes::from_static(b"{}");
        assert!(
            parse_form(Some("application/json"), &body, &limits())
                .await
                .is_none()
        );
        assert!(parse_form(None, &body, &limits()).await.is_none());
    }

    #[test]
    fn query_keeps_order_and_duplicates() {
        let pairs = parse_query(Some("b=2&a=1&b=3&flag"), &limits());
        let flat: Vec<_> = pairs
            .iter()
            .map(|pair| (pair.name.as_str(), pair.value.as_str()))
            .collect();
        assert_eq!(flat, [("b", "2"), ("a", "1"), ("b", "3"), ("flag", "")]);
    }
}
