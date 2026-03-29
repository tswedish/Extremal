//! In-memory cache for hot leaderboard queries.
//!
//! Caches threshold, CID lists, and graph data per leaderboard `n`.
//! Invalidated on admission events. TTL-based expiry as fallback.

use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Default TTL for cached entries.
const DEFAULT_TTL: Duration = Duration::from_secs(30);

/// TTL for graph data (changes less frequently, more expensive to compute).
const GRAPH_TTL: Duration = Duration::from_secs(120);

struct CacheEntry<T> {
    value: T,
    inserted_at: Instant,
    ttl: Duration,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            inserted_at: Instant::now(),
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

/// Cached threshold response for a leaderboard.
struct ThresholdEntry {
    count: i64,
    threshold: Option<Vec<u8>>,
}

/// Cached graph list for a leaderboard page.
struct GraphEntry {
    graphs: Value,
}

/// Thread-safe leaderboard cache.
pub struct LeaderboardCache {
    thresholds: RwLock<HashMap<i32, CacheEntry<ThresholdEntry>>>,
    cids: RwLock<HashMap<i32, CacheEntry<Vec<String>>>>,
    graphs: RwLock<HashMap<(i32, i64, i64), CacheEntry<GraphEntry>>>,
}

impl Default for LeaderboardCache {
    fn default() -> Self {
        Self::new()
    }
}

impl LeaderboardCache {
    pub fn new() -> Self {
        Self {
            thresholds: RwLock::new(HashMap::new()),
            cids: RwLock::new(HashMap::new()),
            graphs: RwLock::new(HashMap::new()),
        }
    }

    // ── Threshold cache ─────────────────────────────────

    /// Get cached threshold, returns (count, threshold_bytes) if fresh.
    pub async fn get_threshold(&self, n: i32) -> Option<(i64, Option<Vec<u8>>)> {
        let cache = self.thresholds.read().await;
        cache.get(&n).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some((entry.value.count, entry.value.threshold.clone()))
            }
        })
    }

    /// Store threshold in cache.
    pub async fn set_threshold(&self, n: i32, count: i64, threshold: Option<Vec<u8>>) {
        let mut cache = self.thresholds.write().await;
        cache.insert(
            n,
            CacheEntry::new(ThresholdEntry { count, threshold }, DEFAULT_TTL),
        );
    }

    // ── CID cache (full sync only, not incremental) ─────

    /// Get cached full CID list (only for requests without `since`).
    pub async fn get_cids(&self, n: i32) -> Option<Vec<String>> {
        let cache = self.cids.read().await;
        cache.get(&n).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.value.clone())
            }
        })
    }

    /// Store full CID list in cache.
    pub async fn set_cids(&self, n: i32, cids: Vec<String>) {
        let mut cache = self.cids.write().await;
        cache.insert(n, CacheEntry::new(cids, DEFAULT_TTL));
    }

    // ── Graph cache ─────────────────────────────────────

    /// Get cached graph data for a page.
    pub async fn get_graphs(&self, n: i32, limit: i64, offset: i64) -> Option<Value> {
        let cache = self.graphs.read().await;
        cache.get(&(n, limit, offset)).and_then(|entry| {
            if entry.is_expired() {
                None
            } else {
                Some(entry.value.graphs.clone())
            }
        })
    }

    /// Store graph data in cache.
    pub async fn set_graphs(&self, n: i32, limit: i64, offset: i64, graphs: Value) {
        let mut cache = self.graphs.write().await;
        cache.insert(
            (n, limit, offset),
            CacheEntry::new(GraphEntry { graphs }, GRAPH_TTL),
        );
    }

    // ── Invalidation ────────────────────────────────────

    /// Invalidate all cached data for a leaderboard `n`.
    /// Called on admission.
    pub async fn invalidate(&self, n: i32) {
        {
            let mut cache = self.thresholds.write().await;
            cache.remove(&n);
        }
        {
            let mut cache = self.cids.write().await;
            cache.remove(&n);
        }
        {
            let mut cache = self.graphs.write().await;
            cache.retain(|&(cn, _, _), _| cn != n);
        }
        tracing::debug!(n, "cache invalidated");
    }
}
