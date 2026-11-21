use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::UpdateError;

/// A new random signing key, as base64. Keep it secret.
pub fn generate_signing_key() -> Result<String, UpdateError> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| UpdateError::BadKey(format!("no randomness: {e}")))?;
    Ok(STANDARD.encode(secret))
}

pub fn parse_signing_key(text: &str) -> Result<SigningKey, UpdateError> {
    let bytes: [u8; 32] = decode(text)?
        .try_into()
        .map_err(|_| UpdateError::BadKey("a signing key is 32 bytes".into()))?;
    Ok(SigningKey::from_bytes(&bytes))
}

pub fn parse_public_key(text: &str) -> Result<VerifyingKey, UpdateError> {
    let bytes: [u8; 32] = decode(text)?
        .try_into()
        .map_err(|_| UpdateError::BadKey("a public key is 32 bytes".into()))?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| UpdateError::BadKey(e.to_string()))
}

/// The public key matching a signing key, as base64.
pub fn public_key_of(key: &SigningKey) -> String {
    STANDARD.encode(key.verifying_key().to_bytes())
}

/// Sign bytes; returns the base64 signature.
pub fn sign(key: &SigningKey, message: &[u8]) -> String {
    STANDARD.encode(key.sign(message).to_bytes())
}

/// Check a base64 signature of `message`.
pub(crate) fn verify(
    key: &VerifyingKey,
    message: &[u8],
    signature: &str,
) -> Result<(), UpdateError> {
    let bytes: [u8; 64] = decode(signature)
        .map_err(|_| UpdateError::BadSignature)?
        .try_into()
        .map_err(|_| UpdateError::BadSignature)?;
    key.verify_strict(message, &Signature::from_bytes(&bytes))
        .map_err(|_| UpdateError::BadSignature)
}

fn decode(text: &str) -> Result<Vec<u8>, UpdateError> {
    STANDARD
        .decode(text.trim())
        .map_err(|e| UpdateError::BadKey(format!("not base64: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let key = parse_signing_key(&generate_signing_key().unwrap()).unwrap();
        let public = parse_public_key(&public_key_of(&key)).unwrap();
        let signature = sign(&key, b"manifest");
        assert!(verify(&public, b"manifest", &signature).is_ok());
        assert!(verify(&public, b"manifesT", &signature).is_err());
        assert!(verify(&public, b"manifest", "not a signature").is_err());

        let other = parse_signing_key(&generate_signing_key().unwrap()).unwrap();
        assert!(verify(&public, b"manifest", &sign(&other, b"manifest")).is_err());
    }
}
