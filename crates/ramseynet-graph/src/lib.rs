pub mod adjacency;
pub mod cid;
pub mod rgxf;

pub use adjacency::AdjacencyMatrix;
pub use cid::compute_cid;
pub use rgxf::{RgxfJson, RgxfError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_creates() {
        let g = AdjacencyMatrix::new(5);
        assert_eq!(g.n(), 5);
        assert_eq!(g.num_edges(), 0);
    }
}
