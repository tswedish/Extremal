use minegraph_graph::adjacency::AdjacencyMatrix;
use minegraph_graph::graph6;
use proptest::prelude::*;

/// Generate a random adjacency matrix with n vertices.
fn arb_adjacency_matrix(max_n: u32) -> impl Strategy<Value = AdjacencyMatrix> {
    (1u32..=max_n).prop_flat_map(|n| {
        let total_bits = AdjacencyMatrix::total_bits(n);
        proptest::collection::vec(any::<bool>(), total_bits).prop_map(move |edges| {
            let mut matrix = AdjacencyMatrix::new(n);
            let mut bit_idx = 0;
            for i in 0..n {
                for j in (i + 1)..n {
                    if bit_idx < edges.len() && edges[bit_idx] {
                        matrix.set_edge(i, j, true);
                    }
                    bit_idx += 1;
                }
            }
            matrix
        })
    })
}

proptest! {
    /// Encoding then decoding any valid adjacency matrix must produce the same matrix.
    #[test]
    fn graph6_roundtrip(matrix in arb_adjacency_matrix(62)) {
        let encoded = graph6::encode(&matrix);
        let decoded = graph6::decode(&encoded).expect("decode should succeed for valid encoding");
        prop_assert_eq!(matrix.n(), decoded.n(), "vertex count mismatch");
        for i in 0..matrix.n() {
            for j in (i + 1)..matrix.n() {
                prop_assert_eq!(
                    matrix.edge(i, j),
                    decoded.edge(i, j),
                    "edge mismatch at ({}, {})", i, j
                );
            }
        }
    }

    /// graph6 encoding is deterministic — encoding the same matrix twice gives the same string.
    #[test]
    fn graph6_encode_deterministic(matrix in arb_adjacency_matrix(30)) {
        let a = graph6::encode(&matrix);
        let b = graph6::encode(&matrix);
        prop_assert_eq!(a, b);
    }

    /// graph6 encoded strings only contain valid ASCII characters (63..126).
    #[test]
    fn graph6_encoded_chars_valid(matrix in arb_adjacency_matrix(62)) {
        let encoded = graph6::encode(&matrix);
        for (i, byte) in encoded.bytes().enumerate() {
            prop_assert!(
                (63..=126).contains(&byte),
                "invalid byte {byte} at position {i} in encoded string"
            );
        }
    }
}

/// Decoding arbitrary bytes should never panic (only return Err).
#[test]
fn graph6_decode_invalid_inputs() {
    // Empty
    assert!(graph6::decode("").is_err());
    // Too short
    assert!(graph6::decode("?").is_ok()); // n=0 is valid
    // Invalid characters (below 63)
    assert!(graph6::decode("\x00\x01\x02").is_err());
    // Very high character (above 126)
    assert!(graph6::decode("\x7F").is_err());
}
