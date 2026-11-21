use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::json;
use url::Url;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::random_bytes;
use whl_secrets::{Keyring, OpenError, SealError, Secret, SecretName};
use whl_store::Store;
use whl_store::notify::{NotifyStoreError, StoredVapid, VAPID_SCOPE, VAPID_SECRET};
use zeroize::Zeroizing;

use crate::push_encrypt::P256DH_BYTES;

pub const MAX_SUBJECT_BYTES: usize = 256;
pub const TOKEN_LIFETIME_SECONDS: u64 = 12 * 3600;

const SCALAR_BYTES: usize = 32;
const SCALAR_ATTEMPTS: usize = 8;
const CREATE_ATTEMPTS: usize = 3;
const HEADER_JSON: &str = r#"{"typ":"JWT","alg":"ES256"}"#;

#[derive(Debug, thiserror::Error)]
pub enum VapidError {
    #[error(
        "the VAPID subject must be a mailto: address or an https URL of at most {MAX_SUBJECT_BYTES} bytes"
    )]
    Subject,
    #[error("the operating system random source failed")]
    Entropy,
    #[error("the stored VAPID key is not a valid P-256 scalar")]
    Corrupt,
    #[error("the stored VAPID public key does not match the sealed private key")]
    Mismatch,
    #[error("the VAPID token could not be signed")]
    Sign,
    #[error("the VAPID storage scope is not valid")]
    Scope,
    #[error("the VAPID key cannot be opened with the configured secret keys: {0}")]
    Open(#[from] OpenError),
    #[error("the VAPID key cannot be sealed: {0}")]
    Seal(#[from] SealError),
    #[error("the VAPID key store failed: {0}")]
    Store(#[from] NotifyStoreError),
    #[error("the VAPID key could not be created")]
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VapidSubject(String);

impl VapidSubject {
    pub fn parse(text: &str) -> Result<Self, VapidError> {
        if text.is_empty()
            || text.len() > MAX_SUBJECT_BYTES
            || text.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(VapidError::Subject);
        }
        let valid = match text.strip_prefix("mailto:") {
            Some(address) => address
                .split_once('@')
                .is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.')),
            None => Url::parse(text).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            }),
        };
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(VapidError::Subject)
        }
    }

    pub fn from_public_url(public_url: &str) -> Result<Self, VapidError> {
        let host = Url::parse(public_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .ok_or(VapidError::Subject)?;
        Self::parse(&format!("https://{host}"))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

pub struct Vapid {
    signing: SigningKey,
    public: [u8; P256DH_BYTES],
    encoded_public: String,
    subject: VapidSubject,
}

impl fmt::Debug for Vapid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vapid")
            .field("public_key", &self.encoded_public)
            .finish_non_exhaustive()
    }
}

impl Vapid {
    pub fn generate(subject: VapidSubject) -> Result<Self, VapidError> {
        for _ in 0..SCALAR_ATTEMPTS {
            let bytes =
                Zeroizing::new(random_bytes::<SCALAR_BYTES>().map_err(|_| VapidError::Entropy)?);
            if let Ok(vapid) = Self::from_scalar(&bytes[..], subject.clone()) {
                return Ok(vapid);
            }
        }
        Err(VapidError::Entropy)
    }

    pub fn from_scalar(scalar: &[u8], subject: VapidSubject) -> Result<Self, VapidError> {
        let signing = SigningKey::from_slice(scalar).map_err(|_| VapidError::Corrupt)?;
        let point = signing.verifying_key().to_sec1_point(false);
        let public =
            <[u8; P256DH_BYTES]>::try_from(point.as_bytes()).map_err(|_| VapidError::Corrupt)?;
        Ok(Self {
            signing,
            public,
            encoded_public: URL_SAFE_NO_PAD.encode(public),
            subject,
        })
    }

    pub fn public_key(&self) -> &str {
        &self.encoded_public
    }

    pub fn public_bytes(&self) -> &[u8; P256DH_BYTES] {
        &self.public
    }

    pub fn authorization(&self, audience: &str, now_unix: u64) -> Result<String, VapidError> {
        let claims = serde_json::to_vec(&json!({
            "aud": audience,
            "exp": now_unix.saturating_add(TOKEN_LIFETIME_SECONDS),
            "sub": self.subject.unpack(),
        }))
        .map_err(|_| VapidError::Sign)?;
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(HEADER_JSON),
            URL_SAFE_NO_PAD.encode(claims)
        );
        let signature: Signature = self
            .signing
            .try_sign(signing_input.as_bytes())
            .map_err(|_| VapidError::Sign)?;
        Ok(format!(
            "vapid t={signing_input}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes().as_slice()),
            self.encoded_public
        ))
    }

    fn scalar_text(&self) -> Zeroizing<String> {
        let scalar = Zeroizing::new(self.signing.to_bytes().to_vec());
        Zeroizing::new(URL_SAFE_NO_PAD.encode(&scalar[..]))
    }
}

pub async fn load_or_create_vapid(
    store: &Store,
    keyring: &Keyring,
    subject: &VapidSubject,
) -> Result<Vapid, VapidError> {
    let scope = EndpointId::parse(VAPID_SCOPE).map_err(|_| VapidError::Scope)?;
    let name = SecretName::parse(VAPID_SECRET).map_err(|_| VapidError::Scope)?;
    for _ in 0..CREATE_ATTEMPTS {
        if let Some(stored) = store.get_vapid_key().await? {
            return open_stored(keyring, &scope, &name, &stored, subject);
        }
        let fresh = Vapid::generate(subject.clone())?;
        let secret =
            Secret::new((*fresh.scalar_text()).clone()).map_err(|_| VapidError::Corrupt)?;
        let sealed = keyring.seal(&scope, &name, &secret)?;
        store
            .insert_vapid_key(fresh.public_bytes(), &sealed)
            .await?;
    }
    Err(VapidError::Missing)
}

fn open_stored(
    keyring: &Keyring,
    scope: &EndpointId,
    name: &SecretName,
    stored: &StoredVapid,
    subject: &VapidSubject,
) -> Result<Vapid, VapidError> {
    let secret = keyring.open(scope, name, &stored.sealed)?;
    let scalar = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(secret.unpack())
            .map_err(|_| VapidError::Corrupt)?,
    );
    let vapid = Vapid::from_scalar(&scalar[..], subject.clone())?;
    if vapid.public_bytes().as_slice() == stored.public_key.as_slice() {
        Ok(vapid)
    } else {
        Err(VapidError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;

    use super::*;

    fn subject() -> VapidSubject {
        VapidSubject::parse("mailto:ops@hooks.example").expect("a valid subject")
    }

    #[test]
    fn the_subject_is_a_mailto_address_or_an_https_url_and_nothing_else() {
        for good in [
            "mailto:ops@hooks.example",
            "https://hooks.example",
            "https://hooks.example/contact",
        ] {
            assert!(VapidSubject::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "http://hooks.example",
            "mailto:nobody",
            "mailto:@hooks.example",
            "https://user@hooks.example",
            "https://hooks.example/a b",
            "javascript:alert(1)",
        ] {
            assert!(VapidSubject::parse(bad).is_err(), "{bad}");
        }
        let long = format!("https://hooks.example/{}", "a".repeat(MAX_SUBJECT_BYTES));
        assert!(VapidSubject::parse(&long).is_err());
        assert_eq!(
            VapidSubject::from_public_url("http://localhost:8080/x")
                .expect("a derived subject")
                .unpack(),
            "https://localhost"
        );
    }

    #[test]
    fn the_authorization_header_carries_a_verifiable_es256_token_and_the_public_key() {
        let vapid = Vapid::generate(subject()).expect("a key pair");
        let header = vapid
            .authorization("https://push.example", 1_700_000_000)
            .expect("a signed token");
        let rest = header.strip_prefix("vapid t=").expect("the scheme");
        let (token, public) = rest.split_once(", k=").expect("the key parameter");
        assert_eq!(public, vapid.public_key());
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).expect("valid base64url");
        let jose: serde_json::Value = serde_json::from_slice(&decode(parts[0])).expect("JSON");
        assert_eq!(jose, json!({ "typ": "JWT", "alg": "ES256" }));
        let claims: serde_json::Value = serde_json::from_slice(&decode(parts[1])).expect("JSON");
        assert_eq!(
            claims,
            json!({
                "aud": "https://push.example",
                "exp": 1_700_000_000_u64 + TOKEN_LIFETIME_SECONDS,
                "sub": "mailto:ops@hooks.example",
            })
        );
        let verifying = VerifyingKey::from_sec1_bytes(&decode(public)).expect("a valid public key");
        let signature = Signature::from_slice(&decode(parts[2])).expect("a raw signature");
        let signed = format!("{}.{}", parts[0], parts[1]);
        assert!(verifying.verify(signed.as_bytes(), &signature).is_ok());
        assert!(verifying.verify(b"other", &signature).is_err());
    }

    #[test]
    fn a_key_survives_its_stored_text_form_and_never_prints_its_scalar() {
        let vapid = Vapid::generate(subject()).expect("a key pair");
        let text = vapid.scalar_text();
        let scalar = URL_SAFE_NO_PAD
            .decode(text.as_str())
            .expect("valid base64url");
        let again = Vapid::from_scalar(&scalar, subject()).expect("the same key");
        assert_eq!(again.public_bytes(), vapid.public_bytes());
        let printed = format!("{vapid:?}");
        assert!(!printed.contains(text.as_str()));
        assert!(printed.contains(vapid.public_key()));
    }
}
