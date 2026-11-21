use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes128Gcm, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToSec1Point;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;
use whl_core::entropy::random_bytes;
use zeroize::Zeroizing;

pub const P256DH_BYTES: usize = 65;
pub const AUTH_SECRET_BYTES: usize = 16;
pub const SALT_BYTES: usize = 16;
pub const RECORD_SIZE: u32 = 4096;
pub const MAX_PLAINTEXT_BYTES: usize = 3993;
pub const MAX_ENCODED_KEY_CHARS: usize = 128;
pub const MIN_RECORD_BYTES: usize = 1024;
pub const PADDING_STEP: usize = 256;

const SCALAR_BYTES: usize = 32;
const CONTENT_KEY_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const SCALAR_ATTEMPTS: usize = 8;
const UNCOMPRESSED_TAG: u8 = 0x04;
const DELIMITER: u8 = 0x02;
const KEY_INFO_PREFIX: &[u8] = b"WebPush: info\0";
const CONTENT_KEY_INFO: &[u8] = b"Content-Encoding: aes128gcm\0";
const NONCE_INFO: &[u8] = b"Content-Encoding: nonce\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("the value is not base64url or is too long")]
    Encoding,
    #[error("the public key must be an uncompressed P-256 point of {P256DH_BYTES} bytes")]
    PublicKeyShape,
    #[error("the public key is not a point on the P-256 curve")]
    PublicKeyPoint,
    #[error("the authentication secret must be {AUTH_SECRET_BYTES} bytes")]
    AuthLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncryptError {
    #[error("the message is longer than {MAX_PLAINTEXT_BYTES} bytes")]
    TooLong,
    #[error("the operating system random source failed")]
    Entropy,
    #[error("the key derivation failed")]
    Derivation,
    #[error("the cipher rejected the input")]
    Cipher,
    #[error("the encrypted body is malformed")]
    Malformed,
}

pub struct SubscriptionKeys {
    public: PublicKey,
    public_bytes: [u8; P256DH_BYTES],
    auth: [u8; AUTH_SECRET_BYTES],
}

impl SubscriptionKeys {
    pub fn parse(p256dh: &[u8], auth: &[u8]) -> Result<Self, KeyError> {
        let public = parse_point(p256dh)?;
        let public_bytes =
            <[u8; P256DH_BYTES]>::try_from(p256dh).map_err(|_| KeyError::PublicKeyShape)?;
        let auth = <[u8; AUTH_SECRET_BYTES]>::try_from(auth).map_err(|_| KeyError::AuthLength)?;
        Ok(Self {
            public,
            public_bytes,
            auth,
        })
    }
}

pub fn decode_p256dh(text: &str) -> Result<Vec<u8>, KeyError> {
    let bytes = decode_base64url(text)?;
    parse_point(&bytes)?;
    Ok(bytes)
}

pub fn decode_auth_secret(text: &str) -> Result<Vec<u8>, KeyError> {
    let bytes = decode_base64url(text)?;
    if bytes.len() == AUTH_SECRET_BYTES {
        Ok(bytes)
    } else {
        Err(KeyError::AuthLength)
    }
}

fn decode_base64url(text: &str) -> Result<Vec<u8>, KeyError> {
    if text.len() > MAX_ENCODED_KEY_CHARS {
        return Err(KeyError::Encoding);
    }
    URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .map_err(|_| KeyError::Encoding)
}

fn parse_point(bytes: &[u8]) -> Result<PublicKey, KeyError> {
    if bytes.len() != P256DH_BYTES || bytes.first() != Some(&UNCOMPRESSED_TAG) {
        return Err(KeyError::PublicKeyShape);
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| KeyError::PublicKeyPoint)
}

pub fn encrypt(keys: &SubscriptionKeys, plaintext: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let sender = random_secret()?;
    let salt = random_bytes::<SALT_BYTES>().map_err(|_| EncryptError::Entropy)?;
    encrypt_with(
        keys,
        plaintext,
        &sender,
        salt,
        padded_record_len(plaintext.len()),
    )
}

pub fn padded_record_len(plaintext_len: usize) -> usize {
    let needed = plaintext_len.saturating_add(1);
    if needed <= MIN_RECORD_BYTES {
        return MIN_RECORD_BYTES;
    }
    (needed.div_ceil(PADDING_STEP) * PADDING_STEP).min(MAX_PLAINTEXT_BYTES + 1)
}

fn random_secret() -> Result<SecretKey, EncryptError> {
    for _ in 0..SCALAR_ATTEMPTS {
        let bytes =
            Zeroizing::new(random_bytes::<SCALAR_BYTES>().map_err(|_| EncryptError::Entropy)?);
        if let Ok(secret) = SecretKey::from_slice(&bytes[..]) {
            return Ok(secret);
        }
    }
    Err(EncryptError::Entropy)
}

struct Derived {
    content_key: Zeroizing<[u8; CONTENT_KEY_BYTES]>,
    nonce: [u8; NONCE_BYTES],
}

fn derive(
    shared: &[u8],
    auth: &[u8],
    receiver: &[u8],
    sender: &[u8],
    salt: &[u8],
) -> Result<Derived, EncryptError> {
    let mut info = Vec::with_capacity(KEY_INFO_PREFIX.len() + receiver.len() + sender.len());
    info.extend_from_slice(KEY_INFO_PREFIX);
    info.extend_from_slice(receiver);
    info.extend_from_slice(sender);
    let mut input = Zeroizing::new([0_u8; SCALAR_BYTES]);
    Hkdf::<Sha256>::new(Some(auth), shared)
        .expand(&info, &mut *input)
        .map_err(|_| EncryptError::Derivation)?;
    let expander = Hkdf::<Sha256>::new(Some(salt), &*input);
    let mut content_key = Zeroizing::new([0_u8; CONTENT_KEY_BYTES]);
    expander
        .expand(CONTENT_KEY_INFO, &mut *content_key)
        .map_err(|_| EncryptError::Derivation)?;
    let mut nonce = [0_u8; NONCE_BYTES];
    expander
        .expand(NONCE_INFO, &mut nonce)
        .map_err(|_| EncryptError::Derivation)?;
    Ok(Derived { content_key, nonce })
}

pub(crate) fn encrypt_with(
    keys: &SubscriptionKeys,
    plaintext: &[u8],
    sender: &SecretKey,
    salt: [u8; SALT_BYTES],
    record_len: usize,
) -> Result<Vec<u8>, EncryptError> {
    if plaintext.len() > MAX_PLAINTEXT_BYTES || record_len > MAX_PLAINTEXT_BYTES + 1 {
        return Err(EncryptError::TooLong);
    }
    if record_len <= plaintext.len() {
        return Err(EncryptError::Malformed);
    }
    let sender_point = sender.public_key().to_sec1_point(false);
    let sender_bytes = sender_point.as_bytes();
    let shared = sender.diffie_hellman(&keys.public);
    let derived = derive(
        shared.raw_secret_bytes().as_slice(),
        &keys.auth,
        &keys.public_bytes,
        sender_bytes,
        &salt,
    )?;
    let mut record = Zeroizing::new(Vec::with_capacity(record_len));
    record.extend_from_slice(plaintext);
    record.push(DELIMITER);
    record.resize(record_len, 0);
    let cipher =
        Aes128Gcm::new_from_slice(&derived.content_key[..]).map_err(|_| EncryptError::Cipher)?;
    let sealed = cipher
        .encrypt(&Nonce::<Aes128Gcm>::from(derived.nonce), record.as_slice())
        .map_err(|_| EncryptError::Cipher)?;
    let mut body = Vec::with_capacity(SALT_BYTES + 4 + 1 + sender_bytes.len() + sealed.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(u8::try_from(sender_bytes.len()).map_err(|_| EncryptError::Malformed)?);
    body.extend_from_slice(sender_bytes);
    body.extend_from_slice(&sealed);
    Ok(body)
}

#[cfg(any(test, feature = "test-support"))]
pub fn decrypt(receiver_scalar: &[u8], auth: &[u8], body: &[u8]) -> Result<Vec<u8>, EncryptError> {
    let header = SALT_BYTES + 4 + 1;
    let salt = body.get(..SALT_BYTES).ok_or(EncryptError::Malformed)?;
    let id_len = usize::from(*body.get(header - 1).ok_or(EncryptError::Malformed)?);
    let sender_bytes = body
        .get(header..header + id_len)
        .ok_or(EncryptError::Malformed)?;
    let sealed = body.get(header + id_len..).ok_or(EncryptError::Malformed)?;
    let receiver = SecretKey::from_slice(receiver_scalar).map_err(|_| EncryptError::Malformed)?;
    let sender = parse_point(sender_bytes).map_err(|_| EncryptError::Malformed)?;
    let receiver_point = receiver.public_key().to_sec1_point(false);
    let shared = receiver.diffie_hellman(&sender);
    let derived = derive(
        shared.raw_secret_bytes().as_slice(),
        auth,
        receiver_point.as_bytes(),
        sender_bytes,
        salt,
    )?;
    let cipher =
        Aes128Gcm::new_from_slice(&derived.content_key[..]).map_err(|_| EncryptError::Cipher)?;
    let record = cipher
        .decrypt(&Nonce::<Aes128Gcm>::from(derived.nonce), sealed)
        .map_err(|_| EncryptError::Cipher)?;
    let end = record
        .iter()
        .rposition(|byte| *byte != 0)
        .ok_or(EncryptError::Malformed)?;
    if record[end] != DELIMITER {
        return Err(EncryptError::Malformed);
    }
    Ok(record[..end].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAINTEXT: &str = "When I grow up, I want to be a watermelon";
    const VECTOR_AS_SCALAR: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
    const VECTOR_UA_SCALAR: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
    const VECTOR_UA_POINT: &str =
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
    const VECTOR_UA_SEED: &str = "BTBZMqHH6r4Tts7J_aSIgg";
    const VECTOR_SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
    const VECTOR_BODY: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

    fn bytes(text: &str) -> Vec<u8> {
        URL_SAFE_NO_PAD.decode(text).expect("valid base64url")
    }

    fn vector_keys() -> SubscriptionKeys {
        SubscriptionKeys::parse(&bytes(VECTOR_UA_POINT), &bytes(VECTOR_UA_SEED))
            .expect("the vector keys are valid")
    }

    fn fresh_receiver() -> (Vec<u8>, SubscriptionKeys, Vec<u8>) {
        let scalar = random_secret().expect("a scalar");
        let point = scalar.public_key().to_sec1_point(false);
        let auth = random_bytes::<AUTH_SECRET_BYTES>().expect("random bytes");
        let keys = SubscriptionKeys::parse(point.as_bytes(), &auth).expect("valid keys");
        (scalar.to_bytes().to_vec(), keys, auth.to_vec())
    }

    #[test]
    fn the_rfc_8291_appendix_a_vector_is_reproduced_byte_for_byte() {
        let sender = SecretKey::from_slice(&bytes(VECTOR_AS_SCALAR)).expect("a valid scalar");
        let salt = <[u8; SALT_BYTES]>::try_from(bytes(VECTOR_SALT)).expect("16 bytes");
        let body = encrypt_with(
            &vector_keys(),
            PLAINTEXT.as_bytes(),
            &sender,
            salt,
            PLAINTEXT.len() + 1,
        )
        .expect("the vector encrypts");
        assert_eq!(body, bytes(VECTOR_BODY));
    }

    #[test]
    fn the_ciphertext_length_hides_the_message_length_by_padding_and_still_decrypts() {
        let (scalar, keys, auth) = fresh_receiver();
        let header = SALT_BYTES + 4 + 1 + P256DH_BYTES;
        let tag = 16;
        for length in [1, 40, 700, MIN_RECORD_BYTES - 1] {
            let body = encrypt(&keys, &vec![b'a'; length]).expect("encrypts");
            assert_eq!(body.len(), header + MIN_RECORD_BYTES + tag, "{length}");
            assert_eq!(
                decrypt(&scalar, &auth, &body).expect("decrypts"),
                vec![b'a'; length]
            );
        }
        for (length, record) in [(MIN_RECORD_BYTES, 1280), (1500, 1536), (3000, 3072)] {
            let body = encrypt(&keys, &vec![b'a'; length]).expect("encrypts");
            assert_eq!(body.len(), header + record + tag, "{length}");
            assert_eq!(
                decrypt(&scalar, &auth, &body).expect("decrypts"),
                vec![b'a'; length]
            );
        }
        assert_eq!(
            padded_record_len(MAX_PLAINTEXT_BYTES),
            MAX_PLAINTEXT_BYTES + 1
        );
    }

    #[test]
    fn the_rfc_8291_appendix_a_body_decrypts_with_the_user_agent_keys() {
        let plaintext = decrypt(
            &bytes(VECTOR_UA_SCALAR),
            &bytes(VECTOR_UA_SEED),
            &bytes(VECTOR_BODY),
        )
        .expect("the vector decrypts");
        assert_eq!(plaintext, PLAINTEXT.as_bytes());
    }

    #[test]
    fn a_fresh_encryption_decrypts_and_never_repeats_its_salt_or_sender_key() {
        let (scalar, keys, auth) = fresh_receiver();
        let first = encrypt(&keys, b"{\"a\":1}").expect("encrypts");
        let second = encrypt(&keys, b"{\"a\":1}").expect("encrypts");
        assert_ne!(first[..SALT_BYTES], second[..SALT_BYTES]);
        assert_ne!(first, second);
        assert_eq!(
            decrypt(&scalar, &auth, &first).expect("decrypts"),
            b"{\"a\":1}"
        );
        let mut tampered = first.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(
            decrypt(&scalar, &auth, &tampered),
            Err(EncryptError::Cipher)
        );
    }

    #[test]
    fn the_message_size_is_bounded_by_one_record() {
        let (scalar, keys, auth) = fresh_receiver();
        let largest = vec![b'a'; MAX_PLAINTEXT_BYTES];
        let body = encrypt(&keys, &largest).expect("the largest message encrypts");
        assert_eq!(body.len(), 4096);
        assert_eq!(decrypt(&scalar, &auth, &body).expect("decrypts"), largest);
        assert_eq!(
            encrypt(&keys, &vec![b'a'; MAX_PLAINTEXT_BYTES + 1]),
            Err(EncryptError::TooLong)
        );
    }

    #[test]
    fn subscription_keys_must_be_a_curve_point_and_a_sixteen_byte_secret() {
        let good = decode_p256dh(VECTOR_UA_POINT).expect("a valid point");
        assert_eq!(good.len(), P256DH_BYTES);
        assert_eq!(decode_p256dh("AAAA"), Err(KeyError::PublicKeyShape));
        assert_eq!(decode_p256dh("not base64!"), Err(KeyError::Encoding));
        let mut off_curve = good.clone();
        off_curve[40] ^= 1;
        assert_eq!(
            decode_p256dh(&URL_SAFE_NO_PAD.encode(&off_curve)),
            Err(KeyError::PublicKeyPoint)
        );
        let mut compressed = good;
        compressed[0] = 0x02;
        assert_eq!(
            decode_p256dh(&URL_SAFE_NO_PAD.encode(&compressed)),
            Err(KeyError::PublicKeyShape)
        );
        assert_eq!(
            decode_auth_secret(VECTOR_UA_SEED).map(|auth| auth.len()),
            Ok(16)
        );
        assert_eq!(decode_auth_secret("AAAA"), Err(KeyError::AuthLength));
        assert_eq!(
            decode_auth_secret(&"A".repeat(MAX_ENCODED_KEY_CHARS + 1)),
            Err(KeyError::Encoding)
        );
    }
}
