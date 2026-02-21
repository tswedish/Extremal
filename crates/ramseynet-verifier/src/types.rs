use ramseynet_graph::RgxfJson;
use ramseynet_types::{GraphCid, Verdict};
use serde::{Deserialize, Serialize};

/// OVWC-1 verification request (stdin JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyRequest {
    pub oras_version: String,
    pub k: u32,
    pub ell: u32,
    pub graph: RgxfJson,
    #[serde(default)]
    pub want_cid: bool,
}

/// OVWC-1 verification response (stdout JSON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness: Option<Vec<u32>>,
}

/// Internal verification result.
#[derive(Clone, Debug)]
pub struct VerifyResult {
    pub verdict: Verdict,
    pub graph_cid: GraphCid,
    pub reason: Option<String>,
    pub witness: Option<Vec<u32>>,
}

impl From<VerifyResult> for VerifyResponse {
    fn from(r: VerifyResult) -> Self {
        VerifyResponse {
            status: r.verdict.to_string(),
            graph_cid: Some(r.graph_cid.to_hex()),
            reason: r.reason,
            witness: r.witness,
        }
    }
}
