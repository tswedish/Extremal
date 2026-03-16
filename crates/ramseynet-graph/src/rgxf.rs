use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adjacency::AdjacencyMatrix;

/// RGXF JSON transport format.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RgxfJson {
    pub n: u32,
    pub encoding: String,
    pub bits_b64: String,
}

/// RGXF binary magic bytes.
const RGXF_MAGIC: &[u8; 4] = b"RGXF";
/// Current binary format version.
const RGXF_VERSION: u8 = 1;

#[derive(Debug, Error)]
pub enum RgxfError {
    #[error("unsupported encoding: {0} (expected utri_b64_v1)")]
    UnsupportedEncoding(String),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid bit vector length for n={n}: expected {expected} bytes, got {got}")]
    BitLengthMismatch { n: u32, expected: usize, got: usize },
    #[error("invalid RGXF binary: {0}")]
    InvalidBinary(String),
}

/// Compute n*(n-1)/2 safely for n < 2.
fn total_bit_count(n: u32) -> usize {
    let n = n as usize;
    if n < 2 {
        return 0;
    }
    n * (n - 1) / 2
}

/// Encode an adjacency matrix to RGXF JSON transport format.
pub fn to_json(matrix: &AdjacencyMatrix) -> RgxfJson {
    RgxfJson {
        n: matrix.n(),
        encoding: "utri_b64_v1".to_string(),
        bits_b64: B64.encode(matrix.packed_bits()),
    }
}

/// Decode an RGXF JSON transport object into an adjacency matrix.
pub fn from_json(json: &RgxfJson) -> Result<AdjacencyMatrix, RgxfError> {
    if json.encoding != "utri_b64_v1" {
        return Err(RgxfError::UnsupportedEncoding(json.encoding.clone()));
    }
    let bits = B64.decode(&json.bits_b64)?;
    let total_bits = total_bit_count(json.n);
    let expected_bytes = total_bits.div_ceil(8);
    if bits.len() != expected_bytes {
        return Err(RgxfError::BitLengthMismatch {
            n: json.n,
            expected: expected_bytes,
            got: bits.len(),
        });
    }
    AdjacencyMatrix::from_bits(json.n, bits).map_err(|e| RgxfError::InvalidBinary(e.to_string()))
}

/// Encode an adjacency matrix to RGXF canonical binary bytes.
///
/// Format: "RGXF" (4 bytes) + version (1 byte, u8) + n (4 bytes LE) + m (4 bytes LE) + packed_bits
pub fn to_canonical_bytes(matrix: &AdjacencyMatrix) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(RGXF_MAGIC);
    out.push(RGXF_VERSION);
    out.extend_from_slice(&matrix.n().to_le_bytes());
    out.extend_from_slice(&(matrix.num_edges() as u32).to_le_bytes());
    out.extend_from_slice(matrix.packed_bits());
    out
}

/// Decode RGXF canonical binary bytes into an adjacency matrix.
pub fn from_canonical_bytes(bytes: &[u8]) -> Result<AdjacencyMatrix, RgxfError> {
    if bytes.len() < 13 {
        return Err(RgxfError::InvalidBinary("too short".into()));
    }
    if &bytes[0..4] != RGXF_MAGIC {
        return Err(RgxfError::InvalidBinary("bad magic".into()));
    }
    if bytes[4] != RGXF_VERSION {
        return Err(RgxfError::InvalidBinary(format!(
            "unsupported version {}",
            bytes[4]
        )));
    }
    let n = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
    // m is stored but we reconstruct from bits
    let _m = u32::from_le_bytes(bytes[9..13].try_into().unwrap());
    let bits = bytes[13..].to_vec();
    let total_bits = total_bit_count(n);
    let expected_bytes = total_bits.div_ceil(8);
    if bits.len() != expected_bytes {
        return Err(RgxfError::BitLengthMismatch {
            n,
            expected: expected_bytes,
            got: bits.len(),
        });
    }
    AdjacencyMatrix::from_bits(n, bits).map_err(|e| RgxfError::InvalidBinary(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(2, 4, true);
        g.set_edge(3, 4, true);

        let json = to_json(&g);
        assert_eq!(json.encoding, "utri_b64_v1");
        assert_eq!(json.n, 5);

        let recovered = from_json(&json).unwrap();
        assert_eq!(g, recovered);
    }

    #[test]
    fn binary_roundtrip() {
        let mut g = AdjacencyMatrix::new(6);
        g.set_edge(0, 2, true);
        g.set_edge(1, 5, true);

        let bytes = to_canonical_bytes(&g);
        assert_eq!(&bytes[0..4], b"RGXF");
        assert_eq!(bytes[4], 1); // version

        let recovered = from_canonical_bytes(&bytes).unwrap();
        assert_eq!(g, recovered);
    }

    #[test]
    fn json_bad_encoding_rejected() {
        let json = RgxfJson {
            n: 3,
            encoding: "bad_format".into(),
            bits_b64: "AA==".into(),
        };
        assert!(from_json(&json).is_err());
    }

    #[test]
    fn empty_graph_roundtrip() {
        let g = AdjacencyMatrix::new(0);
        let json = to_json(&g);
        let recovered = from_json(&json).unwrap();
        assert_eq!(g, recovered);

        let bytes = to_canonical_bytes(&g);
        let recovered2 = from_canonical_bytes(&bytes).unwrap();
        assert_eq!(g, recovered2);
    }
}
