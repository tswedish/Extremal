use serde::{Deserialize, Serialize};

/// Protocol version string.
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// SHA-256 content identifier for a graph artifact.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Identifier for a Ramsey challenge arena.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChallengeId(pub String);

impl ChallengeId {
    /// Create a canonical challenge ID for the given Ramsey parameters.
    pub fn new(k: u32, ell: u32) -> Self {
        Self(format!("ramsey:{k}:{ell}:v1"))
    }
}

impl std::fmt::Display for ChallengeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Ramsey parameters (k, ell): find graphs with no k-clique and no ell-independent-set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RamseyParams {
    pub k: u32,
    pub ell: u32,
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
    fn challenge_id_format() {
        let id = ChallengeId::new(3, 3);
        assert_eq!(id.0, "ramsey:3:3:v1");
    }

    #[test]
    fn graph_cid_hex_roundtrip() {
        let cid = GraphCid([0xab; 32]);
        let hex = cid.to_hex();
        let recovered = GraphCid::from_hex(&hex).unwrap();
        assert_eq!(cid, recovered);
    }

    #[test]
    fn verdict_display() {
        assert_eq!(Verdict::Accepted.to_string(), "accepted");
        assert_eq!(Verdict::Rejected.to_string(), "rejected");
    }
}
