//! Ed25519 signing key management.
//!
//! Key ID = first 16 hex chars of SHA-256(public_key_bytes).
//! Key file format: JSON with public_key, secret_key, key_id, display_name.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

/// Information about a signing key (no secrets).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyInfo {
    pub key_id: String,
    pub public_key_hex: String,
    pub display_name: Option<String>,
}

/// Full key file (stored on disk).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyFile {
    pub key_id: String,
    pub public_key: String,
    pub secret_key: String,
    pub display_name: Option<String>,
}

/// Compute the key ID from a public key (first 16 hex chars of SHA-256).
pub fn compute_key_id(public_key: &VerifyingKey) -> String {
    let hash = Sha256::digest(public_key.as_bytes());
    hex::encode(&hash[..8])
}

/// Compute key ID from hex-encoded public key bytes.
pub fn compute_key_id_from_hex(public_key_hex: &str) -> Result<String> {
    let bytes = hex::decode(public_key_hex).context("invalid hex")?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    let vk = VerifyingKey::from_bytes(&array).context("invalid public key")?;
    Ok(compute_key_id(&vk))
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
    fs::write(path, json)
        .with_context(|| format!("Failed to write key file: {}", path.display()))?;

    Ok(KeyInfo {
        key_id,
        public_key_hex: hex::encode(verifying_key.as_bytes()),
        display_name: display_name.map(|s| s.to_string()),
    })
}

/// Load key info from a JSON file (doesn't expose the secret key).
pub fn load_key_info(path: &Path) -> Result<KeyInfo> {
    let contents = fs::read_to_string(path)
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
pub fn load_signing_key(path: &Path) -> Result<(SigningKey, KeyInfo)> {
    let contents = fs::read_to_string(path)
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

// ── Canonical signing ────────────────────────────────────────────

/// Build the canonical bytes for signing a submission.
/// Sorted JSON keys, no whitespace, deterministic.
pub fn canonical_payload(k: u32, ell: u32, n: u32, bits_b64: &str) -> Vec<u8> {
    // Minimal canonical JSON — sorted keys, no spaces
    format!(
        r#"{{"bits_b64":"{}","encoding":"utri_b64_v1","k":{},"ell":{},"n":{}}}"#,
        bits_b64, k, ell, n
    )
    .into_bytes()
}

/// Sign a canonical submission payload.
pub fn sign_payload(signing_key: &SigningKey, payload: &[u8]) -> String {
    let signature = signing_key.sign(payload);
    hex::encode(signature.to_bytes())
}

/// Verify a signature against a public key and payload.
pub fn verify_signature(public_key_hex: &str, payload: &[u8], signature_hex: &str) -> Result<bool> {
    let pub_bytes = hex::decode(public_key_hex).context("invalid public key hex")?;
    let pub_array: [u8; 32] = pub_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    let verifying_key = VerifyingKey::from_bytes(&pub_array).context("invalid public key")?;

    let sig_bytes = hex::decode(signature_hex).context("invalid signature hex")?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    Ok(verifying_key.verify(payload, &signature).is_ok())
}

// ── Multi-key management ─────────────────────────────────────────

/// List all key files in a directory.
pub fn list_keys(keys_dir: &Path) -> Result<Vec<(String, KeyInfo)>> {
    let mut keys = Vec::new();
    if !keys_dir.exists() {
        return Ok(keys);
    }
    for entry in fs::read_dir(keys_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Ok(info) = load_key_info(&path) {
                let filename = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                keys.push((filename, info));
            }
        }
    }
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(keys)
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

    #[test]
    fn sign_and_verify() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let pub_hex = hex::encode(signing_key.verifying_key().as_bytes());

        let payload = canonical_payload(5, 5, 25, "dGVzdA==");
        let sig_hex = sign_payload(&signing_key, &payload);

        assert!(verify_signature(&pub_hex, &payload, &sig_hex).unwrap());

        // Wrong payload should fail
        let wrong_payload = canonical_payload(5, 5, 24, "dGVzdA==");
        assert!(!verify_signature(&pub_hex, &wrong_payload, &sig_hex).unwrap());
    }

    #[test]
    fn multi_key_list() {
        let dir = tempfile::tempdir().unwrap();
        let keys_dir = dir.path().join("keys");
        fs::create_dir_all(&keys_dir).unwrap();

        generate_and_save(&keys_dir.join("alice.json"), Some("alice")).unwrap();
        generate_and_save(&keys_dir.join("bob.json"), Some("bob")).unwrap();

        let keys = list_keys(&keys_dir).unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].0, "alice");
        assert_eq!(keys[1].0, "bob");
    }
}
