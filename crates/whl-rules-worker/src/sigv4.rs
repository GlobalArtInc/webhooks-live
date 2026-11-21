use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;
use time::macros::format_description;

pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";
pub const SERVICE: &str = "s3";

const TERMINATOR: &str = "aws4_request";
const DATE_BYTES: usize = 8;

#[derive(Debug, Error)]
pub enum SignError {
    #[error("the signing key cannot be derived")]
    Key,
    #[error("the request date is malformed")]
    Date,
}

pub struct SignInput<'a> {
    pub method: &'a str,
    pub canonical_path: &'a str,
    pub canonical_query: &'a str,
    pub headers: &'a [(String, String)],
    pub payload_hash: &'a str,
    pub region: &'a str,
    pub service: &'a str,
    pub amz_date: &'a str,
    pub access_key: &'a str,
    pub secret_key: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub signature: String,
    pub signed_headers: String,
    pub authorization: String,
}

pub fn amz_date(moment: OffsetDateTime) -> String {
    moment
        .to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year][month][day]T[hour][minute][second]Z"
        ))
        .unwrap_or_default()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(Sha256::digest(data).as_slice())
}

pub fn encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(char::from(byte));
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn mac(key: &[u8], data: &[u8]) -> Result<[u8; 32], SignError> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).map_err(|_| SignError::Key)?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().into())
}

pub fn sign(input: &SignInput<'_>) -> Result<Signed, SignError> {
    let date = input.amz_date.get(..DATE_BYTES).ok_or(SignError::Date)?;
    let mut headers: Vec<(String, String)> = input
        .headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    headers.sort_by(|left, right| left.0.cmp(&right.0));
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{}",
        input.method, input.canonical_path, input.canonical_query, input.payload_hash
    );
    let scope = format!("{date}/{}/{}/{TERMINATOR}", input.region, input.service);
    let to_sign = format!(
        "{ALGORITHM}\n{}\n{scope}\n{}",
        input.amz_date,
        sha256_hex(canonical_request.as_bytes())
    );
    let seed = format!("AWS4{}", input.secret_key);
    let date_key = mac(seed.as_bytes(), date.as_bytes())?;
    let region_key = mac(&date_key, input.region.as_bytes())?;
    let service_key = mac(&region_key, input.service.as_bytes())?;
    let signing_key = mac(&service_key, TERMINATOR.as_bytes())?;
    let signature = hex(&mac(&signing_key, to_sign.as_bytes())?);
    let authorization = format!(
        "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        input.access_key
    );
    Ok(Signed {
        signature,
        signed_headers,
        authorization,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn signs_the_published_put_object_example() {
        let access = ["AKIA", "IOSFODNN7", "EXAMPLE"].concat();
        let secret = ["wJalrXUtnFEMI", "/K7MDENG/", "bPxRfiCYEXAMPLEKEY"].concat();
        let payload = sha256_hex(b"Welcome to Amazon S3.");
        assert_eq!(
            payload,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let signed_headers = headers(&[
            ("Date", "Fri, 24 May 2013 00:00:00 GMT"),
            ("Host", "examplebucket.s3.amazonaws.com"),
            ("x-amz-content-sha256", &payload),
            ("x-amz-date", "20130524T000000Z"),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
        ]);
        let signed = sign(&SignInput {
            method: "PUT",
            canonical_path: &encode_path("/test$file.text"),
            canonical_query: "",
            headers: &signed_headers,
            payload_hash: &payload,
            region: "us-east-1",
            service: SERVICE,
            amz_date: "20130524T000000Z",
            access_key: &access,
            secret_key: &secret,
        })
        .unwrap();
        assert_eq!(
            signed.signature,
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
        assert_eq!(
            signed.signed_headers,
            "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class"
        );
    }

    #[test]
    fn encodes_every_byte_outside_the_unreserved_set_but_keeps_the_separators() {
        assert_eq!(encode_path("/a b/é+~.-_/z"), "/a%20b/%C3%A9%2B~.-_/z");
    }

    #[test]
    fn formats_the_request_date_in_utc() {
        let moment = OffsetDateTime::from_unix_timestamp(1_369_353_600).unwrap();
        assert_eq!(amz_date(moment), "20130524T000000Z");
    }
}
