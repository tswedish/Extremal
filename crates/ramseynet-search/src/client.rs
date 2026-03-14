use ramseynet_graph::RgxfJson;
use serde::{Deserialize, Serialize};

use crate::error::SearchError;

/// Leaderboard graphs response from the server.
#[derive(Debug, Deserialize)]
struct LeaderboardGraphsResponse {
    graphs: Vec<RgxfJson>,
}

/// Threshold info returned by the server.
#[derive(Debug, Deserialize)]
pub struct ThresholdResponse {
    pub entry_count: u32,
    pub capacity: u32,
    pub worst_tier1_max: Option<u64>,
    pub worst_tier1_min: Option<u64>,
    pub worst_tier2_aut: Option<f64>,
    pub worst_tier3_cid: Option<String>,
}

/// Submit response from the server.
#[derive(Debug, Deserialize)]
pub struct SubmitResponse {
    pub graph_cid: String,
    pub verdict: String,
    pub admitted: Option<bool>,
    pub rank: Option<u32>,
    pub reason: Option<String>,
    pub witness: Option<Vec<u32>>,
}

#[derive(Serialize)]
struct SubmitRequest {
    k: u32,
    ell: u32,
    n: u32,
    graph: RgxfJson,
}

/// Async HTTP client for the RamseyNet server.
pub struct ServerClient {
    base_url: String,
    client: reqwest::Client,
}

impl ServerClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Fetch the admission threshold for a (k, ell, n) leaderboard.
    pub async fn get_threshold(
        &self,
        k: u32,
        ell: u32,
        n: u32,
    ) -> Result<ThresholdResponse, SearchError> {
        let url = format!(
            "{}/api/leaderboards/{}/{}/{}/threshold",
            self.base_url, k, ell, n
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::ServerError(format!("{status}: {body}")));
        }

        let info: ThresholdResponse = resp.json().await?;
        Ok(info)
    }

    /// Fetch RGXF graphs from a leaderboard (top `limit` entries).
    pub async fn get_leaderboard_graphs(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        limit: u32,
    ) -> Result<Vec<RgxfJson>, SearchError> {
        let url = format!(
            "{}/api/leaderboards/{}/{}/{}/graphs?limit={}",
            self.base_url, k, ell, n, limit
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::ServerError(format!("{status}: {body}")));
        }

        let body: LeaderboardGraphsResponse = resp.json().await?;
        Ok(body.graphs)
    }

    /// Fetch leaderboard entry CIDs for local cache.
    pub async fn get_leaderboard_cids(
        &self,
        k: u32,
        ell: u32,
        n: u32,
    ) -> Result<Vec<String>, SearchError> {
        let url = format!(
            "{}/api/leaderboards/{}/{}/{}",
            self.base_url, k, ell, n
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::ServerError(format!("{status}: {body}")));
        }

        let body: serde_json::Value = resp.json().await?;
        let cids = body["entries"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e["graph_cid"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(cids)
    }

    /// Submit a graph to the server.
    pub async fn submit(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        graph: RgxfJson,
    ) -> Result<SubmitResponse, SearchError> {
        let url = format!("{}/api/submit", self.base_url);
        let body = SubmitRequest { k, ell, n, graph };

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::ServerError(format!("{status}: {body}")));
        }

        let submit_resp: SubmitResponse = resp.json().await?;
        Ok(submit_resp)
    }
}
