//! `barkit1.<payload>.<signature>` signed envelopes (Ed25519).

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::de::DeserializeOwned;

use crate::model::{Feed, License};

const PREFIX: &str = "barkit1";

#[derive(Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    Malformed,
    BadSignature,
    BadPayload,
    WrongType,
    WrongApp,
    Expired,
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EnvelopeError::Malformed => "malformed envelope",
            EnvelopeError::BadSignature => "signature does not verify",
            EnvelopeError::BadPayload => "payload is not valid JSON for this type",
            EnvelopeError::WrongType => "envelope has the wrong type",
            EnvelopeError::WrongApp => "envelope is for a different app",
            EnvelopeError::Expired => "envelope has expired",
        };
        f.write_str(s)
    }
}

impl std::error::Error for EnvelopeError {}

/// Parses a public key given as base64url (32 raw bytes).
pub fn parse_public_key(b64: &str) -> Option<VerifyingKey> {
    let bytes = B64.decode(b64.trim()).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

/// Verifies the signature and returns the decoded payload.
pub fn open<T: DeserializeOwned>(token: &str, key: &VerifyingKey) -> Result<T, EnvelopeError> {
    let mut parts = token.trim().split('.');
    let (prefix, payload, sig) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), Some(c), None) => (a, b, c),
        _ => return Err(EnvelopeError::Malformed),
    };
    if prefix != PREFIX {
        return Err(EnvelopeError::Malformed);
    }
    let sig_bytes = B64.decode(sig).map_err(|_| EnvelopeError::Malformed)?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| EnvelopeError::Malformed)?;
    let signed = format!("{PREFIX}.{payload}");
    key.verify(signed.as_bytes(), &sig)
        .map_err(|_| EnvelopeError::BadSignature)?;
    let json = B64.decode(payload).map_err(|_| EnvelopeError::Malformed)?;
    serde_json::from_slice(&json).map_err(|_| EnvelopeError::BadPayload)
}

pub fn open_license(
    token: &str,
    key: &VerifyingKey,
    app_id: &str,
    now: i64,
) -> Result<License, EnvelopeError> {
    let lic: License = open(token, key)?;
    if lic.typ != "license" {
        return Err(EnvelopeError::WrongType);
    }
    if lic.app_id != app_id {
        return Err(EnvelopeError::WrongApp);
    }
    if now >= lic.exp {
        return Err(EnvelopeError::Expired);
    }
    Ok(lic)
}

pub fn open_feed(
    token: &str,
    key: &VerifyingKey,
    app_id: &str,
    now: i64,
) -> Result<Feed, EnvelopeError> {
    let feed: Feed = open(token, key)?;
    if feed.typ != "feed" {
        return Err(EnvelopeError::WrongType);
    }
    if feed.app_id != app_id {
        return Err(EnvelopeError::WrongApp);
    }
    if now >= feed.exp {
        return Err(EnvelopeError::Expired);
    }
    Ok(feed)
}

/// Signing lives on the server; this exists for tests and local tooling.
pub fn seal(payload: &serde_json::Value, key: &ed25519_dalek::SigningKey) -> String {
    use ed25519_dalek::Signer;
    let body = B64.encode(serde_json::to_vec(payload).expect("serialisable"));
    let signed = format!("{PREFIX}.{body}");
    let sig = key.sign(signed.as_bytes());
    format!("{signed}.{}", B64.encode(sig.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    fn key() -> SigningKey {
        SigningKey::generate(&mut rand::rngs::OsRng)
    }

    fn license(app: &str, exp: i64) -> serde_json::Value {
        json!({"typ":"license","lic":"lic_1","dev":"dev_1","app_id":app,"plan":"lifetime","iat":0,"exp":exp})
    }

    #[test]
    fn round_trip() {
        let k = key();
        let t = seal(&license("app_1", 100), &k);
        let lic = open_license(&t, &k.verifying_key(), "app_1", 50).unwrap();
        assert_eq!(lic.plan, "lifetime");
    }

    #[test]
    fn rejects_other_key() {
        let t = seal(&license("app_1", 100), &key());
        assert_eq!(
            open_license(&t, &key().verifying_key(), "app_1", 50),
            Err(EnvelopeError::BadSignature)
        );
    }

    #[test]
    fn rejects_tampered_payload() {
        let k = key();
        let t = seal(&license("app_1", 100), &k);
        let parts: Vec<&str> = t.split('.').collect();
        let forged = B64.encode(serde_json::to_vec(&license("app_2", 100)).unwrap());
        let t2 = format!("{}.{}.{}", parts[0], forged, parts[2]);
        assert_eq!(
            open_license(&t2, &k.verifying_key(), "app_2", 50),
            Err(EnvelopeError::BadSignature)
        );
    }

    #[test]
    fn rejects_wrong_app_and_expired() {
        let k = key();
        let t = seal(&license("app_1", 100), &k);
        assert_eq!(
            open_license(&t, &k.verifying_key(), "app_2", 50),
            Err(EnvelopeError::WrongApp)
        );
        assert_eq!(
            open_license(&t, &k.verifying_key(), "app_1", 100),
            Err(EnvelopeError::Expired)
        );
    }

    #[test]
    fn public_key_round_trip() {
        let k = key();
        let b64 = B64.encode(k.verifying_key().to_bytes());
        assert_eq!(parse_public_key(&b64), Some(k.verifying_key()));
    }
}
