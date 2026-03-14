use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::rngs::SmallRng;
use rand::SeedableRng;
use ramseynet_graph::{compute_cid, rgxf, AdjacencyMatrix};
use ramseynet_verifier::scoring::compute_score_canonical;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::client::ServerClient;
use crate::error::SearchError;
use crate::search::Searcher;
use crate::viz::{
    CollectorObserver, Discovery, DiscoveryCollector, KnownCids, VizHandle, VizObserver,
};

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

// ── Periodic submission ─────────────────────────────────────────────

/// Drain the collector and submit discoveries to the server, skipping
/// any whose canonical CID is already known (from prior rounds, server
/// leaderboard, or earlier submissions in this batch).
#[allow(clippy::too_many_arguments)]
async fn submit_discoveries(
    client: &ServerClient,
    collector: &DiscoveryCollector,
    known: &KnownCids,
    k: u32,
    ell: u32,
    n: u32,
    viz_handle: &Option<Arc<VizHandle>>,
    strategy: &str,
) -> (usize, usize) {
    let discoveries = collector.drain();
    if discoveries.is_empty() {
        return (0, 0);
    }

    let mut submitted = 0usize;
    let mut admitted = 0usize;

    for discovery in &discoveries {
        let cid_hex = discovery.cid.to_hex();

        // Skip if already known (leaderboard, prior submission, or prior discovery)
        if known.contains(&discovery.cid) {
            continue;
        }

        let rgxf_json = rgxf::to_json(&discovery.graph);
        match client.submit(k, ell, n, rgxf_json).await {
            Ok(resp) => {
                let was_admitted = resp.admitted.unwrap_or(false);
                info!(
                    graph_cid = %resp.graph_cid,
                    verdict = %resp.verdict,
                    admitted = was_admitted,
                    rank = ?resp.rank,
                    "submitted graph"
                );
                // Mark as known so we never re-submit
                known.insert_hex(&resp.graph_cid);
                submitted += 1;

                if was_admitted {
                    admitted += 1;
                    info!("admitted to leaderboard! rank={}", resp.rank.unwrap_or(0));
                    if let Some(ref vh) = viz_handle {
                        vh.submit_discovery(
                            &discovery.graph, n, strategy, 0,
                            true, discovery.score.clone(),
                        );
                    }
                }
            }
            Err(e) => {
                error!(graph_cid = %cid_hex, "submit failed: {e}");
            }
        }
    }

    (submitted, admitted)
}

// ── Main worker loop ────────────────────────────────────────────────

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

    // Persistent known-CID set: survives across rounds, strategies, and
    // periodic submission batches. Prevents re-submitting any graph that
    // has been seen before (on the leaderboard, submitted, or discovered).
    let known = KnownCids::new();

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

        // Refresh known CIDs from server leaderboard
        match client.get_leaderboard_cids(k, ell, target_n).await {
            Ok(cids) => {
                known.add_from_hex(&cids);
                info!(known = known.len(), "refreshed known CIDs from server");
            }
            Err(e) => {
                warn!("failed to fetch leaderboard CIDs: {e}");
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

            // Collector with cross-round dedup via known CIDs
            let collector = DiscoveryCollector::with_known(100, known.clone());
            let collector_for_search = collector.clone();
            let collector_for_submit = collector.clone();
            let n = target_n;
            let searcher = Arc::clone(searcher);
            let mut search_rng = SmallRng::from_rng(&mut rng).unwrap();
            let viz = viz_handle.clone();

            // Shared cancellation flag — set on Ctrl+C so the search
            // bails out within ~100 iterations instead of running to max_iters.
            let cancel_flag = Arc::new(AtomicBool::new(false));
            let cancel_for_search = cancel_flag.clone();

            // Spawn the search in a blocking thread
            let mut search_handle = tokio::task::spawn_blocking(move || {
                let viz_obs = viz.map(VizObserver::new);
                let obs = CollectorObserver::with_cancel(
                    collector_for_search, viz_obs, cancel_for_search,
                );
                searcher.search(n, k, ell, max_iters, &mut search_rng, &obs)
            });

            // While search is running, periodically drain and submit discoveries
            let submit_interval = Duration::from_secs(30);
            let strategy_name = strategy.to_string();
            let mut last_submit = Instant::now();
            let mut shutting_down = false;

            loop {
                tokio::select! {
                    result = &mut search_handle => {
                        // Search completed (normally or via cancellation)
                        let result = result.unwrap();
                        let elapsed = start.elapsed();

                        if shutting_down {
                            info!(
                                strategy,
                                iterations = result.iterations,
                                elapsed_ms = elapsed.as_millis() as u64,
                                "search interrupted by shutdown"
                            );
                        }

                        // If final result is valid, add it to the collector
                        if result.valid {
                            let score_result = compute_score_canonical(&result.graph);
                            let canonical_cid = compute_cid(&score_result.canonical_graph);
                            collector_for_submit.push(Discovery {
                                graph: score_result.canonical_graph.clone(),
                                score: score_result.score.clone(),
                                cid: canonical_cid,
                            });

                            if let Some(ref vh) = viz_handle {
                                if let Some(entry) = vh.submit_discovery(
                                    &result.graph, target_n, strategy, result.iterations,
                                    false, score_result.score,
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
                            } else if !shutting_down {
                                info!(
                                    strategy,
                                    target_n,
                                    iterations = result.iterations,
                                    edges = result.graph.num_edges(),
                                    elapsed_ms = elapsed.as_millis() as u64,
                                    "found valid graph!"
                                );
                            }
                        } else if !shutting_down {
                            warn!(
                                strategy,
                                target_n,
                                iterations = result.iterations,
                                elapsed_ms = elapsed.as_millis() as u64,
                                "no valid graph found"
                            );
                        }

                        // Final drain + submit any remaining discoveries
                        let (sub, adm) = submit_discoveries(
                            &client, &collector_for_submit, &known,
                            k, ell, target_n, &viz_handle, &strategy_name,
                        ).await;
                        if sub > 0 {
                            info!(submitted = sub, admitted = adm, "final submission batch");
                            found = true;
                        }

                        if shutting_down {
                            info!("shutdown complete — submitted remaining discoveries");
                            return Ok(());
                        }

                        break; // Move to next strategy
                    }
                    _ = shutdown.changed(), if !shutting_down => {
                        // Ctrl+C received — signal the search to stop ASAP
                        info!("shutdown signal received, cancelling search...");
                        cancel_flag.store(true, Ordering::Relaxed);
                        shutting_down = true;
                        // Don't break — wait for the search to finish (should be fast)
                        // The next iteration will pick up the search_handle completing.
                    }
                    _ = tokio::time::sleep(submit_interval.saturating_sub(last_submit.elapsed())) => {
                        // Check shutdown between periodic submissions
                        if *shutdown.borrow() && !shutting_down {
                            info!("shutdown signal received, cancelling search...");
                            cancel_flag.store(true, Ordering::Relaxed);
                            shutting_down = true;
                        }

                        // Periodic drain + submit while search is still running
                        let (sub, adm) = submit_discoveries(
                            &client, &collector_for_submit, &known,
                            k, ell, target_n, &viz_handle, &strategy_name,
                        ).await;
                        if sub > 0 {
                            info!(submitted = sub, admitted = adm, "periodic submission batch");
                            found = true;
                            consecutive_failures = 0;
                        }
                        last_submit = Instant::now();
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
                    _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
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

    // Even in offline mode, keep a known-CID set so we don't re-discover
    // the same isomorphism classes across rounds.
    let known = KnownCids::new();

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
            let collector = DiscoveryCollector::with_known(100, known.clone());
            let collector_for_search = collector.clone();
            let (result, score) = tokio::task::spawn_blocking(move || {
                let viz_obs = viz.map(VizObserver::new);
                let obs = CollectorObserver::new(collector_for_search, viz_obs);
                let result = searcher.search(n, k, ell, max_iters, &mut search_rng, &obs);
                let score = if result.valid {
                    let sr = compute_score_canonical(&result.graph);
                    Some(sr.score)
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
