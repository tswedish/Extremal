//! Core newtypes for Extremal.
//!
//! This crate has zero internal dependencies and defines the fundamental
//! types shared across the entire system.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Protocol version for API compatibility checks.
pub const PROTOCOL_VERSION: &str = "1.0.0";

/// Crate version from Cargo.toml (semver).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit SHA, captured at build time.
pub const BUILD_COMMIT: &str = env!("BUILD_COMMIT");

/// Combined version string for display: "0.2.0 (abc12345)".
pub fn build_version() -> String {
    if BUILD_COMMIT == "unknown" {
        VERSION.to_string()
    } else {
        format!("{VERSION} ({BUILD_COMMIT})")
    }
}

// ---------------------------------------------------------------------------
// GraphCid — content address for a graph (blake3 hash of canonical graph6)
// ---------------------------------------------------------------------------

/// A 32-byte content identifier for a graph, computed as
/// `blake3(canonical_graph6_bytes)`.
///
/// Two graphs are considered identical if and only if they share a CID,
/// which requires them to be isomorphic (same canonical labeling).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphCid(pub [u8; 32]);

impl GraphCid {
    /// Create a CID from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Decode a CID from a hex string.
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Encode the CID as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for GraphCid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GraphCid({})", &self.to_hex()[..16])
    }
}

impl fmt::Display for GraphCid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for GraphCid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for GraphCid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// KeyId — identity derived from a public key
// ---------------------------------------------------------------------------

/// An identity key ID — first 16 hex characters of `blake3(public_key_bytes)`.
///
/// This is 8 bytes of entropy (64 bits), sufficient for collision resistance
/// within a single Extremal network.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyId(pub String);

impl KeyId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({})", self.0)
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

/// The outcome of a graph verification / scoring request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Accepted,
    Rejected,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cid_hex_roundtrip() {
        let bytes = [42u8; 32];
        let cid = GraphCid::from_bytes(bytes);
        let hex_str = cid.to_hex();
        let decoded = GraphCid::from_hex(&hex_str).unwrap();
        assert_eq!(cid, decoded);
    }

    #[test]
    fn cid_ordering() {
        let a = GraphCid::from_bytes([0u8; 32]);
        let b = GraphCid::from_bytes([1u8; 32]);
        assert!(a < b);
    }

    #[test]
    fn cid_serde_json() {
        let cid = GraphCid::from_bytes([0xAB; 32]);
        let json = serde_json::to_string(&cid).unwrap();
        let decoded: GraphCid = serde_json::from_str(&json).unwrap();
        assert_eq!(cid, decoded);
    }

    #[test]
    fn verdict_serde() {
        let json = serde_json::to_string(&Verdict::Accepted).unwrap();
        assert_eq!(json, "\"accepted\"");
        let decoded: Verdict = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, Verdict::Accepted);
    }

    #[test]
    fn key_id_display() {
        let kid = KeyId::new("3f8a1b2c4d5e6f70");
        assert_eq!(kid.to_string(), "3f8a1b2c4d5e6f70");
    }
}
