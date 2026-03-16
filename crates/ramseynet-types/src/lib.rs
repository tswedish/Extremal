use serde::{Deserialize, Serialize};

/// Protocol version string.
pub const PROTOCOL_VERSION: &str = "0.2.0";

/// SHA-256 content identifier for a graph artifact.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphCid(pub [u8; 32]);

impl GraphCid {
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| hex::FromHexError::InvalidStringLength)?;
        Ok(Self(arr))
    }
}

impl std::fmt::Display for GraphCid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Ramsey parameters (k, ell): find graphs with no k-clique and no ell-independent-set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RamseyParams {
    pub k: u32,
    pub ell: u32,
}

impl RamseyParams {
    /// Create canonical parameters with k <= ell (since R(k,l) = R(l,k)).
    pub fn canonical(k: u32, ell: u32) -> Self {
        if k <= ell {
            Self { k, ell }
        } else {
            Self { k: ell, ell: k }
        }
    }
}

/// Verification verdict.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Accepted,
    Rejected,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::Accepted => write!(f, "accepted"),
            Verdict::Rejected => write!(f, "rejected"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_cid_hex_roundtrip() {
        let cid = GraphCid([0xab; 32]);
        let hex = cid.to_hex();
        let recovered = GraphCid::from_hex(&hex).unwrap();
        assert_eq!(cid, recovered);
    }

    #[test]
    fn graph_cid_ord() {
        let a = GraphCid([0x00; 32]);
        let b = GraphCid([0xff; 32]);
        assert!(a < b);
    }

    #[test]
    fn ramsey_params_canonical() {
        let p = RamseyParams::canonical(4, 3);
        assert_eq!(p.k, 3);
        assert_eq!(p.ell, 4);

        let p2 = RamseyParams::canonical(3, 4);
        assert_eq!(p2.k, 3);
        assert_eq!(p2.ell, 4);
    }

    #[test]
    fn verdict_display() {
        assert_eq!(Verdict::Accepted.to_string(), "accepted");
        assert_eq!(Verdict::Rejected.to_string(), "rejected");
    }
}
