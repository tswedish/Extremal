//! HTTP client for talking to the MineGraph server.

use minegraph_identity::{Identity, canonical_payload};
use serde::{Deserialize, Serialize};

/// HTTP client for the MineGraph server API.
pub struct ServerClient {
    base_url: String,
    http: reqwest::Client,
    identity: Option<Identity>,
}

#[derive(Debug, Serialize)]
struct SubmitRequest {
    n: u32,
    graph6: String,
    key_id: String,
    signature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitResponse {
    pub cid: String,
    pub verdict: String,
    pub admitted: bool,
    pub rank: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct ThresholdResponse {
    pub n: i32,
    pub count: i64,
    pub capacity: i32,
    pub threshold_score_bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CidsResponse {
    pub cids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GraphEntry {
    pub graph6: String,
    #[allow(dead_code)]
    pub rank: i32,
}

#[derive(Debug, Deserialize)]
struct GraphsResponse {
    pub graphs: Vec<GraphEntry>,
}

impl ServerClient {
    /// Create a new client for the given server URL.
    pub fn new(base_url: &str, identity: Option<Identity>) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            identity,
        }
    }

    /// Get the key_id of the signing identity, if any.
    pub fn key_id(&self) -> Option<String> {
        self.identity
            .as_ref()
            .map(|id| id.key_id.as_str().to_string())
    }

    /// Health check.
    pub async fn health(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.http
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await?
            .json()
            .await
    }

    /// Get admission threshold for leaderboard n.
    pub async fn get_threshold(&self, n: u32) -> Result<ThresholdResponse, reqwest::Error> {
        self.http
            .get(format!("{}/api/leaderboards/{n}/threshold", self.base_url))
            .send()
            .await?
            .json()
            .await
    }

    /// Get CIDs on the leaderboard (optionally since a timestamp).
    pub async fn get_cids(
        &self,
        n: u32,
        since: Option<&str>,
    ) -> Result<CidsResponse, reqwest::Error> {
        let mut url = format!("{}/api/leaderboards/{n}/cids", self.base_url);
        if let Some(since) = since {
            url.push_str(&format!("?since={}", urlencoded(since)));
        }
        self.http.get(&url).send().await?.json().await
    }

    /// Get graph6 data from the leaderboard for seeding.
    pub async fn get_graphs(
        &self,
        n: u32,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<String>, reqwest::Error> {
        let resp: GraphsResponse = self
            .http
            .get(format!(
                "{}/api/leaderboards/{n}/graphs?limit={limit}&offset={offset}",
                self.base_url
            ))
            .send()
            .await?
            .json()
            .await?;
        Ok(resp.graphs.into_iter().map(|g| g.graph6).collect())
    }

    /// Submit a graph to the server.
    pub async fn submit(
        &self,
        n: u32,
        graph6: &str,
        metadata: Option<&serde_json::Value>,
    ) -> Result<SubmitResponse, String> {
        let identity = self
            .identity
            .as_ref()
            .ok_or_else(|| "no signing identity configured".to_string())?;

        let payload = canonical_payload(n, graph6);
        let signature = identity.sign(&payload);

        let req = SubmitRequest {
            n,
            graph6: graph6.to_string(),
            key_id: identity.key_id.as_str().to_string(),
            signature,
            metadata: metadata.cloned(),
        };

        let resp = self
            .http
            .post(format!("{}/api/submit", self.base_url))
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("submit request failed: {e}"))?;

        if resp.status().is_success() {
            resp.json()
                .await
                .map_err(|e| format!("submit response parse failed: {e}"))
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(format!("submit rejected ({status}): {text}"))
        }
    }
}

/// Simple URL encoding for query parameters.
fn urlencoded(s: &str) -> String {
    s.replace(':', "%3A").replace('+', "%2B")
}
