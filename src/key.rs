use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::Sha256;

/// A newtype wrapper for a 32 byte secret key.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretKey(pub [u8; 32]);

impl std::ops::Deref for SecretKey {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SecretKey {
    /// Generate a new random key
    pub fn random() -> anyhow::Result<Self> {
        let mut secret_key = [0; 32];
        getrandom::getrandom(&mut secret_key)?;
        Ok(Self(secret_key))
    }

    /// Creates a [`SecretKey`] from a hex value.
    pub fn from_hex(input: &str) -> Option<Self> {
        Some(Self(from_hex(input)?))
    }

    /// Returns an HMAC instance for this secret key for signing.
    pub fn hmac(&self) -> Hmac<Sha256> {
        Hmac::<Sha256>::new_from_slice(&self.0).unwrap()
    }

    /// Returns a hex representation of the secret key.
    pub fn hex(&self) -> String {
        to_hex(&self.0)
    }

    /// Verifies a signed HMAC payload in the following format:
    ///
    /// `<base64 payload>.<base64 signature>`.
    ///
    /// This method verifies and returns the decoded payload.
    pub fn verify<T: DeserializeOwned>(&self, value: &str) -> Option<T> {
        let (payload, signature) = value.split_once('.')?;
        let payload = BASE64_URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
        let signature = BASE64_URL_SAFE_NO_PAD.decode(signature.as_bytes()).ok()?;
        let mut hmac = self.hmac();
        hmac.update(&payload);
        hmac.verify_slice(&signature).ok()?;
        serde_json::from_slice::<T>(&payload).ok()
    }

    /// Signs the payload with the expected `<base64 payload>.<base64 signature>` format.
    pub fn sign<T: Serialize>(&self, value: &T) -> serde_json::Result<String> {
        let mut mac = self.hmac();
        let json = serde_json::to_string(value)?;
        mac.update(json.as_bytes());
        let signature = mac.finalize().into_bytes();
        let mut buffer = String::with_capacity(json.len());
        BASE64_URL_SAFE_NO_PAD.encode_string(json.as_bytes(), &mut buffer);
        buffer.push('.');
        BASE64_URL_SAFE_NO_PAD.encode_string(signature, &mut buffer);
        Ok(buffer)
    }
}

/// An alias mainly to aid in readability when the secret key is used as a nonce.
pub type Nonce = SecretKey;

// The reason why hex is used and not base64 is because the base64 library
// triggers a length error despite the length being *exactly* the same.

const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Converts a byte slice to lowercase hex
pub fn to_hex(s: &[u8]) -> String {
    let mut buf = String::with_capacity(s.len() * 2);
    for ch in s {
        buf.push(HEX_LOWER[(ch >> 4) as usize] as char);
        buf.push(HEX_LOWER[(ch & 0x0F) as usize] as char);
    }
    buf
}

const fn hex_to_byte(ch: u8) -> Option<u8> {
    match ch {
        b'0'..=b'9' => Some(ch - b'0'),
        b'a'..=b'f' => Some(ch - b'a' + 10),
        b'A'..=b'F' => Some(ch - b'A' + 10),
        _ => None,
    }
}

/// Converts a lowercase hex string to a byte array
pub fn from_hex<const N: usize>(input: &str) -> Option<[u8; N]> {
    if input.len() != N * 2 {
        return None;
    }
    let bytes = input
        .as_bytes()
        .chunks_exact(2)
        .map(|bytes| Some(hex_to_byte(bytes[0])? + hex_to_byte(bytes[1])?));
    let mut output = [0; N];
    for (elem, byte) in output.iter_mut().zip(bytes) {
        *elem = byte?;
    }
    Some(output)
}

impl Serialize for SecretKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for SecretKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom("invalid hex string (must be 64 characters)"))
    }
}
