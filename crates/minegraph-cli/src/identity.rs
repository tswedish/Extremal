//! Ed25519 signing key management.
//!
//! Key ID = first 16 hex chars of SHA-256(public_key_bytes).
//! Key file format: JSON with public_key, secret_key, key_id, display_name.

use std::path::Path;

use anyhow::{Context, Result};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// Information about a signing key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyInfo {
    pub key_id: String,
    pub public_key_hex: String,
    pub display_name: Option<String>,
}

/// Full key file (stored on disk).
#[derive(serde::Serialize, serde::Deserialize)]
struct KeyFile {
    key_id: String,
    public_key: String,
    secret_key: String,
    display_name: Option<String>,
}

/// Compute the key ID from a public key (first 16 hex chars of SHA-256).
pub fn compute_key_id(public_key: &VerifyingKey) -> String {
    let hash = Sha256::digest(public_key.as_bytes());
    hex::encode(&hash[..8]) // 8 bytes = 16 hex chars
}

/// Generate a new keypair and save to a JSON file.
pub fn generate_and_save(path: &Path, display_name: Option<&str>) -> Result<KeyInfo> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let key_id = compute_key_id(&verifying_key);

    let key_file = KeyFile {
        key_id: key_id.clone(),
        public_key: hex::encode(verifying_key.as_bytes()),
        secret_key: hex::encode(signing_key.to_bytes()),
        display_name: display_name.map(|s| s.to_string()),
    };

    let json = serde_json::to_string_pretty(&key_file)?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write key file: {}", path.display()))?;

    Ok(KeyInfo {
        key_id,
        public_key_hex: hex::encode(verifying_key.as_bytes()),
        display_name: display_name.map(|s| s.to_string()),
    })
}

/// Load key info from a JSON file (doesn't expose the secret key).
pub fn load_key_info(path: &Path) -> Result<KeyInfo> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read key file: {}", path.display()))?;
    let key_file: KeyFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse key file: {}", path.display()))?;

    Ok(KeyInfo {
        key_id: key_file.key_id,
        public_key_hex: key_file.public_key,
        display_name: key_file.display_name,
    })
}

/// Load the full signing key from a JSON file.
#[allow(dead_code)] // Used by worker signing integration (upcoming)
pub fn load_signing_key(path: &Path) -> Result<(SigningKey, KeyInfo)> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read key file: {}", path.display()))?;
    let key_file: KeyFile = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse key file: {}", path.display()))?;

    let secret_bytes = hex::decode(&key_file.secret_key).context("Invalid hex in secret_key")?;
    let secret_array: [u8; 32] = secret_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("secret_key must be 32 bytes"))?;
    let signing_key = SigningKey::from_bytes(&secret_array);

    let info = KeyInfo {
        key_id: key_file.key_id,
        public_key_hex: key_file.public_key,
        display_name: key_file.display_name,
    };

    Ok((signing_key, info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.json");

        let info = generate_and_save(&path, Some("test-key")).unwrap();
        assert_eq!(info.key_id.len(), 16);
        assert_eq!(info.public_key_hex.len(), 64);
        assert_eq!(info.display_name.as_deref(), Some("test-key"));

        let loaded = load_key_info(&path).unwrap();
        assert_eq!(loaded.key_id, info.key_id);
        assert_eq!(loaded.public_key_hex, info.public_key_hex);

        let (signing_key, loaded_info) = load_signing_key(&path).unwrap();
        assert_eq!(loaded_info.key_id, info.key_id);
        // Verify the signing key produces the same public key
        let pub_hex = hex::encode(signing_key.verifying_key().as_bytes());
        assert_eq!(pub_hex, info.public_key_hex);
    }

    #[test]
    fn key_id_is_deterministic() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_key = signing_key.verifying_key();
        let id1 = compute_key_id(&pub_key);
        let id2 = compute_key_id(&pub_key);
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);
    }
}
