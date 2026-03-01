use std::sync::{Arc, Mutex};
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::SeedableRng;
use ramseynet_graph::{compute_cid, rgxf, AdjacencyMatrix};
use ramseynet_verifier::scoring::compute_score;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::client::ServerClient;
use crate::error::SearchError;
use crate::search::Searcher;
use crate::viz::{CollectorObserver, DiscoveryCollector, VizHandle, VizObserver};

/// Configuration for the worker loop.
pub struct WorkerConfig {
    pub k: u32,
    pub ell: u32,
    pub n: u32,
    pub max_iters: u64,
    pub no_backoff: bool,
    pub offline: bool,
    /// Shared pool for leaderboard-seeded init strategy. When `Some`, the
    /// worker refreshes this pool from the server each round.
    pub leaderboard_pool: Option<Arc<Mutex<Vec<AdjacencyMatrix>>>>,
}

/// Run the search worker loop.
pub async fn run_worker(
    client: ServerClient,
    searchers: Vec<Box<dyn Searcher>>,
    config: WorkerConfig,
    mut shutdown: watch::Receiver<bool>,
    viz_handle: Option<Arc<VizHandle>>,
) -> Result<(), SearchError> {
    if config.offline {
        return run_worker_offline(searchers, config, shutdown, viz_handle).await;
    }

    let searchers: Vec<Arc<dyn Searcher>> = searchers.into_iter().map(Arc::from).collect();
    let mut rng = SmallRng::from_entropy();
    let mut consecutive_failures = 0u32;
    let k = config.k;
    let ell = config.ell;
    let target_n = config.n;

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            info!("shutdown signal received, exiting");
            return Ok(());
        }

        // Fetch threshold
        match client.get_threshold(k, ell, target_n).await {
            Ok(info) => {
                info!(
                    k, ell, target_n,
                    entries = info.entry_count,
                    capacity = info.capacity,
                    "fetched leaderboard threshold"
                );
            }
            Err(e) => {
                warn!("failed to fetch threshold: {e}");
            }
        }

        // Refresh leaderboard pool if using leaderboard init
        if let Some(ref pool) = config.leaderboard_pool {
            match client.get_leaderboard_graphs(k, ell, target_n, 20).await {
                Ok(rgxfs) => {
                    let graphs: Vec<AdjacencyMatrix> = rgxfs
                        .iter()
                        .filter_map(|r| rgxf::from_json(r).ok())
                        .collect();
                    let count = graphs.len();
                    *pool.lock().unwrap() = graphs;
                    info!(count, "refreshed leaderboard seed pool");
                }
                Err(e) => {
                    warn!("failed to fetch leaderboard graphs: {e}");
                }
            }
        }

        info!(k, ell, target_n, "starting search round");

        let mut found = false;

        for searcher in &searchers {
            if *shutdown.borrow() {
                info!("shutdown signal received, exiting");
                return Ok(());
            }

            let start = Instant::now();
            let strategy = searcher.name();
            let max_iters = config.max_iters;

            info!(strategy, target_n, max_iters, "running search");

            // Run search in blocking thread with discovery collector
            let n = target_n;
            let searcher = Arc::clone(searcher);
            let mut search_rng = SmallRng::from_rng(&mut rng).unwrap();
            let viz = viz_handle.clone();
            let collector = DiscoveryCollector::new();
            let collector_for_search = collector.clone();
            let (result, score) = tokio::task::spawn_blocking(move || {
                let viz_obs = viz.map(VizObserver::new);
                let obs = CollectorObserver::new(collector_for_search, viz_obs);
                let result = searcher.search(n, k, ell, max_iters, &mut search_rng, &obs);
                let score = if result.valid {
                    Some(compute_score(&result.graph, &compute_cid(&result.graph)))
                } else {
                    None
                };
                (result, score)
            })
            .await
            .unwrap();

            let elapsed = start.elapsed();

            // Add the final result to collector (dedup + bounded insert handled internally)
            if let Some(ref score) = score {
                let final_cid = compute_cid(&result.graph);
                collector.push(crate::viz::Discovery {
                    graph: result.graph.clone(),
                    score: score.clone(),
                    cid: final_cid,
                });
            }

            // Drain best discoveries (already sorted, bounded to 100, CID-deduped)
            let discoveries = collector.drain();

            if let Some(score) = score {
                // Submit final result to viz leaderboard
                if let Some(ref vh) = viz_handle {
                    if let Some(entry) = vh.submit_discovery(
                        &result.graph, target_n, strategy, result.iterations,
                        false, score,
                    ) {
                        info!(
                            strategy,
                            target_n,
                            iterations = result.iterations,
                            edges = result.graph.num_edges(),
                            elapsed_ms = elapsed.as_millis() as u64,
                            omega = entry.score.omega,
                            alpha = entry.score.alpha,
                            c_omega = entry.score.c_omega,
                            c_alpha = entry.score.c_alpha,
                            aut_order = entry.score.aut_order,
                            rank = entry.rank,
                            "found valid graph!"
                        );
                    }
                } else {
                    info!(
                        strategy,
                        target_n,
                        iterations = result.iterations,
                        edges = result.graph.num_edges(),
                        elapsed_ms = elapsed.as_millis() as u64,
                        "found valid graph!"
                    );
                }
            } else if discoveries.is_empty() {
                warn!(
                    strategy,
                    target_n,
                    iterations = result.iterations,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "no valid graph found"
                );
            }

            // Submit all discoveries to server
            if !discoveries.is_empty() {
                info!(
                    strategy,
                    count = discoveries.len(),
                    "submitting all discoveries to server"
                );
                for discovery in &discoveries {
                    let rgxf_json = rgxf::to_json(&discovery.graph);
                    match client.submit(k, ell, target_n, rgxf_json).await {
                        Ok(resp) => {
                            let admitted = resp.admitted.unwrap_or(false);
                            info!(
                                graph_cid = %resp.graph_cid,
                                verdict = %resp.verdict,
                                admitted,
                                rank = ?resp.rank,
                                "submitted graph"
                            );
                            if admitted {
                                info!("admitted to leaderboard! rank={}", resp.rank.unwrap_or(0));
                                if let Some(ref vh) = viz_handle {
                                    vh.submit_discovery(
                                        &discovery.graph, target_n, strategy, result.iterations,
                                        true, discovery.score.clone(),
                                    );
                                }
                            }
                            consecutive_failures = 0;
                            found = true;
                        }
                        Err(e) => {
                            error!(graph_cid = %discovery.cid.to_hex(), "submit failed: {e}");
                            consecutive_failures += 1;
                        }
                    }
                }
            }
        }

        if !found {
            consecutive_failures += 1;

            if config.no_backoff {
                warn!(consecutive_failures, target_n, "all strategies failed, retrying immediately");
            } else {
                let backoff_secs = (2u64.pow(consecutive_failures.min(5))).min(60);
                warn!(
                    consecutive_failures,
                    backoff_secs,
                    target_n,
                    "all strategies failed, backing off"
                );

                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                    _ = shutdown.changed() => {
                        info!("shutdown signal received during backoff");
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Offline worker loop — no server, searches continuously and pins to viz.
async fn run_worker_offline(
    searchers: Vec<Box<dyn Searcher>>,
    config: WorkerConfig,
    shutdown: watch::Receiver<bool>,
    viz_handle: Option<Arc<VizHandle>>,
) -> Result<(), SearchError> {
    let k = config.k;
    let ell = config.ell;
    let target_n = config.n;

    info!(
        k, ell, target_n,
        "starting offline search (no server)"
    );

    let searchers: Vec<Arc<dyn Searcher>> = searchers.into_iter().map(Arc::from).collect();
    let mut rng = SmallRng::from_entropy();
    let mut round = 0u64;

    loop {
        if *shutdown.borrow() {
            info!("shutdown signal received, exiting");
            return Ok(());
        }

        round += 1;

        for searcher in &searchers {
            if *shutdown.borrow() {
                info!("shutdown signal received, exiting");
                return Ok(());
            }

            let start = Instant::now();
            let strategy = searcher.name();
            let max_iters = config.max_iters;

            let n = target_n;
            let searcher = Arc::clone(searcher);
            let mut search_rng = SmallRng::from_rng(&mut rng).unwrap();
            let viz = viz_handle.clone();
            let collector = DiscoveryCollector::new();
            let collector_for_search = collector.clone();
            let (result, score) = tokio::task::spawn_blocking(move || {
                let viz_obs = viz.map(VizObserver::new);
                let obs = CollectorObserver::new(collector_for_search, viz_obs);
                let result = searcher.search(n, k, ell, max_iters, &mut search_rng, &obs);
                let score = if result.valid {
                    Some(compute_score(&result.graph, &compute_cid(&result.graph)))
                } else {
                    None
                };
                (result, score)
            })
            .await
            .unwrap();

            let elapsed = start.elapsed();
            // Discoveries already forwarded to viz via CollectorObserver; just drain to drop
            let _discoveries = collector.drain();

            if let Some(score) = score {
                if let Some(ref vh) = viz_handle {
                    if let Some(entry) = vh.submit_discovery(
                        &result.graph, target_n, strategy, result.iterations,
                        false, score,
                    ) {
                        info!(
                            strategy,
                            target_n,
                            round,
                            iterations = result.iterations,
                            edges = result.graph.num_edges(),
                            elapsed_ms = elapsed.as_millis() as u64,
                            omega = entry.score.omega,
                            alpha = entry.score.alpha,
                            c_omega = entry.score.c_omega,
                            c_alpha = entry.score.c_alpha,
                            aut_order = entry.score.aut_order,
                            rank = entry.rank,
                            "found valid graph (offline)"
                        );
                    }
                } else {
                    info!(
                        strategy,
                        target_n,
                        round,
                        iterations = result.iterations,
                        edges = result.graph.num_edges(),
                        elapsed_ms = elapsed.as_millis() as u64,
                        "found valid graph (offline)"
                    );
                }
            } else {
                warn!(
                    strategy,
                    target_n,
                    round,
                    iterations = result.iterations,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "no valid graph found"
                );
            }
        }
    }
}
