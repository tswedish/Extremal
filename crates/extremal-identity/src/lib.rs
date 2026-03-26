//! Ed25519 identity and signing for Extremal.
//!
//! This is the **single source of truth** for all signing operations.
//! The server, worker, and CLI all depend on this crate instead of
//! duplicating the signing logic.

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use extremal_types::KeyId;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid secret key bytes")]
    InvalidSecretKey,
    #[error("invalid public key bytes")]
    InvalidPublicKey,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("hex decode error: {0}")]
    HexDecode(#[from] hex::FromHexError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Serializable key file format.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyFile {
    pub key_id: String,
    pub public_key: String, // hex
    pub secret_key: String, // hex
    #[serde(default)]
    pub display_name: Option<String>,
}

/// A loaded signing identity.
pub struct Identity {
    pub key_id: KeyId,
    pub signing_key: SigningKey,
    pub display_name: Option<String>,
}

impl Identity {
    /// Generate a fresh Ed25519 identity.
    pub fn generate(display_name: Option<String>) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let key_id = compute_key_id(&signing_key.verifying_key());
        Self {
            key_id,
            signing_key,
            display_name,
        }
    }

    /// Get the verifying (public) key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Sign a canonical payload and return the signature as hex.
    pub fn sign(&self, payload: &[u8]) -> String {
        let sig = self.signing_key.sign(payload);
        hex::encode(sig.to_bytes())
    }

    /// Serialize to a key file.
    pub fn to_key_file(&self) -> KeyFile {
        KeyFile {
            key_id: self.key_id.0.clone(),
            public_key: hex::encode(self.verifying_key().as_bytes()),
            secret_key: hex::encode(self.signing_key.to_bytes()),
            display_name: self.display_name.clone(),
        }
    }

    /// Load from a key file.
    pub fn from_key_file(kf: &KeyFile) -> Result<Self, IdentityError> {
        let secret_bytes = hex::decode(&kf.secret_key)?;
        if secret_bytes.len() != 32 {
            return Err(IdentityError::InvalidSecretKey);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&secret_bytes);
        let signing_key = SigningKey::from_bytes(&arr);
        let key_id = compute_key_id(&signing_key.verifying_key());
        Ok(Self {
            key_id,
            signing_key,
            display_name: kf.display_name.clone(),
        })
    }

    /// Load from a JSON file path.
    pub fn load(path: &std::path::Path) -> Result<Self, IdentityError> {
        let data = std::fs::read_to_string(path)?;
        let kf: KeyFile = serde_json::from_str(&data)?;
        Self::from_key_file(&kf)
    }

    /// Save to a JSON file path.
    pub fn save(&self, path: &std::path::Path) -> Result<(), IdentityError> {
        let kf = self.to_key_file();
        let json = serde_json::to_string_pretty(&kf)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Compute the key ID from a verifying (public) key.
///
/// `KeyId = first 16 hex chars of blake3(public_key_bytes)` = 8 bytes = 64 bits.
pub fn compute_key_id(public_key: &VerifyingKey) -> KeyId {
    let hash = blake3::hash(public_key.as_bytes());
    let hex_str = hex::encode(hash.as_bytes());
    KeyId::new(&hex_str[..16])
}

/// Compute key ID from a hex-encoded public key string.
pub fn compute_key_id_from_hex(public_key_hex: &str) -> Result<KeyId, IdentityError> {
    let bytes = hex::decode(public_key_hex)?;
    if bytes.len() != 32 {
        return Err(IdentityError::InvalidPublicKey);
    }
    let vk = VerifyingKey::from_bytes(bytes.as_slice().try_into().unwrap())
        .map_err(|_| IdentityError::InvalidPublicKey)?;
    Ok(compute_key_id(&vk))
}

/// Verify an Ed25519 signature.
///
/// - `public_key_hex`: 32-byte public key as hex
/// - `payload`: the canonical bytes that were signed
/// - `signature_hex`: 64-byte Ed25519 signature as hex
pub fn verify_signature(
    public_key_hex: &str,
    payload: &[u8],
    signature_hex: &str,
) -> Result<bool, IdentityError> {
    let pk_bytes = hex::decode(public_key_hex)?;
    if pk_bytes.len() != 32 {
        return Err(IdentityError::InvalidPublicKey);
    }
    let vk = VerifyingKey::from_bytes(pk_bytes.as_slice().try_into().unwrap())
        .map_err(|_| IdentityError::InvalidPublicKey)?;

    let sig_bytes = hex::decode(signature_hex)?;
    if sig_bytes.len() != 64 {
        return Err(IdentityError::InvalidSignature);
    }
    let sig = ed25519_dalek::Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());

    Ok(vk.verify(payload, &sig).is_ok())
}

/// Build canonical payload bytes for signing a graph submission.
///
/// The canonical payload is: `n` as 4 bytes LE + graph6 string bytes.
/// This is deterministic for a given canonical graph.
pub fn canonical_payload(n: u32, graph6: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + graph6.len());
    buf.extend_from_slice(&n.to_le_bytes());
    buf.extend_from_slice(graph6.as_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_sign_verify() {
        let identity = Identity::generate(Some("test".into()));
        let payload = b"hello world";
        let sig_hex = identity.sign(payload);

        let pk_hex = hex::encode(identity.verifying_key().as_bytes());
        let valid = verify_signature(&pk_hex, payload, &sig_hex).unwrap();
        assert!(valid);

        // Wrong payload should fail
        let wrong = verify_signature(&pk_hex, b"wrong", &sig_hex).unwrap();
        assert!(!wrong);
    }

    #[test]
    fn key_id_is_deterministic() {
        let identity = Identity::generate(None);
        let pk = identity.verifying_key();
        let id1 = compute_key_id(&pk);
        let id2 = compute_key_id(&pk);
        assert_eq!(id1, id2);
        assert_eq!(id1.as_str().len(), 16);
    }

    #[test]
    fn key_file_roundtrip() {
        let identity = Identity::generate(Some("test-name".into()));
        let kf = identity.to_key_file();
        let loaded = Identity::from_key_file(&kf).unwrap();
        assert_eq!(identity.key_id, loaded.key_id);
        assert_eq!(loaded.display_name.as_deref(), Some("test-name"));

        // Should produce same signatures
        let payload = b"test payload";
        let sig1 = identity.sign(payload);
        let sig2 = loaded.sign(payload);
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn compute_key_id_from_hex_works() {
        let identity = Identity::generate(None);
        let pk_hex = hex::encode(identity.verifying_key().as_bytes());
        let kid = compute_key_id_from_hex(&pk_hex).unwrap();
        assert_eq!(kid, identity.key_id);
    }

    #[test]
    fn canonical_payload_deterministic() {
        let p1 = canonical_payload(25, "DcG?_");
        let p2 = canonical_payload(25, "DcG?_");
        assert_eq!(p1, p2);

        // Different n = different payload
        let p3 = canonical_payload(26, "DcG?_");
        assert_ne!(p1, p3);
    }
}
