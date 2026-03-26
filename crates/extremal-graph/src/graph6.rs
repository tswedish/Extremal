//! graph6 encoding and decoding.
//!
//! The graph6 format is a standard compact representation for undirected
//! simple graphs. See <https://users.cecs.anu.edu.au/~bdm/data/formats.txt>.
//!
//! ## Format summary
//!
//! A graph6 string encodes an undirected graph on n vertices:
//! 1. **N(n)**: encode the vertex count n
//!    - If n <= 62: single byte `n + 63`
//!    - If n <= 258047: four bytes `126, (n>>12)+63, ((n>>6)&63)+63, (n&63)+63`
//!    - If n <= 68719476735: eight bytes (not implemented — we only need n <= 64)
//! 2. **R(x)**: encode the upper triangle of the adjacency matrix
//!    - Bits are read in column-major order: for j=1..n-1, for i=0..j-1, bit(i,j)
//!    - Pad to a multiple of 6 bits, then each 6-bit group becomes a byte + 63

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::adjacency::AdjacencyMatrix;

#[derive(Debug, Error)]
pub enum Graph6Error {
    #[error("empty graph6 string")]
    Empty,
    #[error("invalid graph6 character: {0}")]
    InvalidChar(u8),
    #[error("graph6 vertex count too large: {0} (max supported: 62)")]
    TooLarge(u64),
    #[error("graph6 string too short for {n} vertices")]
    TooShort { n: u32 },
    #[error("graph6 string has extra bytes")]
    ExtraBytes,
}

/// JSON transport format for graphs. Uses graph6 encoding.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphJson {
    pub n: u32,
    pub graph6: String,
}

/// Encode an adjacency matrix into a graph6 string.
///
/// Only supports n <= 62 (single-byte vertex count encoding).
pub fn encode(matrix: &AdjacencyMatrix) -> String {
    let n = matrix.n();
    assert!(n <= 62, "graph6 encode only supports n <= 62");

    let mut bytes = Vec::new();

    // N(n): single byte for n <= 62
    bytes.push(n as u8 + 63);

    // R(x): upper triangle in column-major order, packed into 6-bit groups
    let mut bits = Vec::new();
    for j in 1..n {
        for i in 0..j {
            bits.push(matrix.edge(i, j));
        }
    }

    // Pad to multiple of 6
    while bits.len() % 6 != 0 {
        bits.push(false);
    }

    // Pack into 6-bit groups
    for chunk in bits.chunks(6) {
        let mut byte: u8 = 0;
        for (k, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << (5 - k);
            }
        }
        bytes.push(byte + 63);
    }

    // graph6 is ASCII-safe, so this is valid UTF-8
    String::from_utf8(bytes).expect("graph6 bytes are valid ASCII")
}

/// Decode a graph6 string into an adjacency matrix.
///
/// Only supports n <= 62 (single-byte vertex count encoding).
pub fn decode(s: &str) -> Result<AdjacencyMatrix, Graph6Error> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(Graph6Error::Empty);
    }

    // Validate all characters are in range 63..=126
    for &b in bytes {
        if !(63..=126).contains(&b) {
            return Err(Graph6Error::InvalidChar(b));
        }
    }

    // Decode N(n)
    let (n, rest) = if bytes[0] != 126 {
        // Single byte: n = byte - 63
        let n = (bytes[0] - 63) as u32;
        (n, &bytes[1..])
    } else {
        // Multi-byte: not supported for our use case
        return Err(Graph6Error::TooLarge(0));
    };

    if n > 62 {
        return Err(Graph6Error::TooLarge(n as u64));
    }

    // Compute expected number of bits and 6-bit groups
    let total_bits = AdjacencyMatrix::total_bits(n);
    let expected_groups = total_bits.div_ceil(6);

    if rest.len() < expected_groups {
        return Err(Graph6Error::TooShort { n });
    }
    if rest.len() > expected_groups {
        return Err(Graph6Error::ExtraBytes);
    }

    // Decode R(x): unpack 6-bit groups into bits
    let mut bits = Vec::with_capacity(expected_groups * 6);
    for &b in rest {
        let val = b - 63;
        for k in 0..6 {
            bits.push((val >> (5 - k)) & 1 == 1);
        }
    }

    // Build adjacency matrix from column-major upper triangle
    let mut matrix = AdjacencyMatrix::new(n);
    let mut idx = 0;
    for j in 1..n {
        for i in 0..j {
            if idx < bits.len() && bits[idx] {
                matrix.set_edge(i, j, true);
            }
            idx += 1;
        }
    }

    Ok(matrix)
}

/// Convert a [`GraphJson`] to an [`AdjacencyMatrix`].
pub fn from_json(json: &GraphJson) -> Result<AdjacencyMatrix, Graph6Error> {
    let matrix = decode(&json.graph6)?;
    if matrix.n() != json.n {
        return Err(Graph6Error::TooShort { n: json.n });
    }
    Ok(matrix)
}

/// Convert an [`AdjacencyMatrix`] to a [`GraphJson`].
pub fn to_json(matrix: &AdjacencyMatrix) -> GraphJson {
    GraphJson {
        n: matrix.n(),
        graph6: encode(matrix),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_roundtrip() {
        // n=0: just the vertex count byte
        let g = AdjacencyMatrix::new(0);
        let s = encode(&g);
        assert_eq!(s, "?"); // 0 + 63 = 63 = '?'
        let decoded = decode(&s).unwrap();
        assert_eq!(decoded.n(), 0);
    }

    #[test]
    fn single_vertex() {
        let g = AdjacencyMatrix::new(1);
        let s = encode(&g);
        assert_eq!(s, "@"); // 1 + 63 = 64 = '@'
        let decoded = decode(&s).unwrap();
        assert_eq!(decoded.n(), 1);
        assert_eq!(decoded.num_edges(), 0);
    }

    #[test]
    fn k5_complete_graph() {
        // K5: all 10 edges present
        let mut g = AdjacencyMatrix::new(5);
        for i in 0..5u32 {
            for j in (i + 1)..5 {
                g.set_edge(i, j, true);
            }
        }
        let s = encode(&g);
        let decoded = decode(&s).unwrap();
        assert_eq!(decoded.n(), 5);
        assert_eq!(decoded.num_edges(), 10);
        for i in 0..5u32 {
            for j in (i + 1)..5 {
                assert!(decoded.edge(i, j));
            }
        }
    }

    #[test]
    fn c5_cycle() {
        // C5: 0-1, 1-2, 2-3, 3-4, 4-0
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 1, true);
        g.set_edge(1, 2, true);
        g.set_edge(2, 3, true);
        g.set_edge(3, 4, true);
        g.set_edge(0, 4, true);
        let s = encode(&g);
        let decoded = decode(&s).unwrap();
        assert_eq!(g, decoded);
    }

    #[test]
    fn graph6_roundtrip_various_sizes() {
        for n in 0..=20u32 {
            let g = AdjacencyMatrix::new(n);
            let s = encode(&g);
            let decoded = decode(&s).unwrap();
            assert_eq!(g, decoded, "empty graph n={n}");
        }
    }

    #[test]
    fn petersen_graph() {
        // Petersen graph on 10 vertices (well-known test case)
        let mut g = AdjacencyMatrix::new(10);
        // Outer cycle: 0-1-2-3-4-0
        let outer = [(0, 1), (1, 2), (2, 3), (3, 4), (0, 4)];
        // Inner pentagram: 5-7, 7-9, 9-6, 6-8, 8-5
        let inner = [(5, 7), (7, 9), (6, 9), (6, 8), (5, 8)];
        // Spokes: 0-5, 1-6, 2-7, 3-8, 4-9
        let spokes = [(0, 5), (1, 6), (2, 7), (3, 8), (4, 9)];
        for (i, j) in outer.iter().chain(inner.iter()).chain(spokes.iter()) {
            g.set_edge(*i, *j, true);
        }
        assert_eq!(g.num_edges(), 15);

        let s = encode(&g);
        let decoded = decode(&s).unwrap();
        assert_eq!(g, decoded);
    }

    #[test]
    fn json_roundtrip() {
        let mut g = AdjacencyMatrix::new(5);
        g.set_edge(0, 2, true);
        g.set_edge(1, 3, true);
        let json = to_json(&g);
        assert_eq!(json.n, 5);
        let decoded = from_json(&json).unwrap();
        assert_eq!(g, decoded);
    }

    #[test]
    fn invalid_chars_rejected() {
        assert!(decode("abc").is_err()); // 'a' = 97, but valid range is 63-126
        // Actually 'a' = 97 which is in range. Let's use a real invalid char.
        assert!(decode("\x01").is_err());
    }

    #[test]
    fn empty_string_rejected() {
        assert!(decode("").is_err());
    }
}
