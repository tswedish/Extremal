use ramseynet_graph::RgxfJson;
use serde::{Deserialize, Serialize};

use crate::error::WorkerError;

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
    pub worst_goodman_gap: Option<u64>,
    pub worst_tier2_aut: Option<f64>,
    pub worst_tier3_cid: Option<String>,
}

/// Incremental CID sync response from the server.
#[derive(Debug, Deserialize)]
pub struct CidsSyncResponse {
    pub total: u32,
    pub cids: Vec<String>,
    pub last_updated: Option<String>,
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
#[derive(Clone)]
pub struct ServerClient {
    base_url: String,
    client: reqwest::Client,
}

impl ServerClient {
    pub fn new(base_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        }
    }

    /// Fetch the admission threshold for a (k, ell, n) leaderboard.
    pub async fn get_threshold(
        &self,
        k: u32,
        ell: u32,
        n: u32,
    ) -> Result<ThresholdResponse, WorkerError> {
        let url = format!(
            "{}/api/leaderboards/{}/{}/{}/threshold",
            self.base_url, k, ell, n
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::ServerError(format!("{status}: {body}")));
        }

        let info: ThresholdResponse = resp.json().await?;
        Ok(info)
    }

    /// Fetch RGXF graphs from a leaderboard with pagination.
    pub async fn get_leaderboard_graphs(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RgxfJson>, WorkerError> {
        let url = format!(
            "{}/api/leaderboards/{}/{}/{}/graphs?limit={}&offset={}",
            self.base_url, k, ell, n, limit, offset
        );
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::ServerError(format!("{status}: {body}")));
        }

        let body: LeaderboardGraphsResponse = resp.json().await?;
        Ok(body.graphs)
    }

    /// Fetch leaderboard CIDs incrementally.
    pub async fn get_leaderboard_cids_since(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        since: Option<&str>,
    ) -> Result<CidsSyncResponse, WorkerError> {
        let mut url = format!(
            "{}/api/leaderboards/{}/{}/{}/cids",
            self.base_url, k, ell, n
        );
        if let Some(since) = since {
            let encoded: String = since
                .chars()
                .map(|c| match c {
                    ':' => "%3A".to_string(),
                    '+' => "%2B".to_string(),
                    _ => c.to_string(),
                })
                .collect();
            url.push_str(&format!("?since={encoded}"));
        }
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::ServerError(format!("{status}: {body}")));
        }

        let sync_resp: CidsSyncResponse = resp.json().await?;
        Ok(sync_resp)
    }

    /// Submit a graph to the server.
    pub async fn submit(
        &self,
        k: u32,
        ell: u32,
        n: u32,
        graph: RgxfJson,
    ) -> Result<SubmitResponse, WorkerError> {
        let url = format!("{}/api/submit", self.base_url);
        let body = SubmitRequest { k, ell, n, graph };

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::ServerError(format!("{status}: {body}")));
        }

        let submit_resp: SubmitResponse = resp.json().await?;
        Ok(submit_resp)
    }
}
