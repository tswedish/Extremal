use std::sync::Arc;
use std::time::Instant;

use rand::rngs::SmallRng;
use rand::SeedableRng;
use ramseynet_graph::rgxf;
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::client::ServerClient;
use crate::error::SearchError;
use crate::search::Searcher;
use crate::viz::{compute_rarity, NoOpObserver, VizHandle, VizObserver};

/// Configuration for the worker loop.
pub struct WorkerConfig {
    pub challenge_id: String,
    pub start_n: Option<u32>,
    pub max_iters: u64,
    pub no_backoff: bool,
    pub offline: bool,
}

/// Parse k, ell from a challenge ID like "ramsey:3:4:v1".
fn parse_challenge_params(challenge_id: &str) -> Result<(u32, u32), SearchError> {
    let parts: Vec<&str> = challenge_id.split(':').collect();
    if parts.len() != 4 || parts[0] != "ramsey" {
        return Err(SearchError::Other(format!(
            "invalid challenge ID format: {challenge_id} (expected ramsey:K:L:vN)"
        )));
    }
    let k: u32 = parts[1].parse().map_err(|_| {
        SearchError::Other(format!("invalid k in challenge ID: {}", parts[1]))
    })?;
    let ell: u32 = parts[2].parse().map_err(|_| {
        SearchError::Other(format!("invalid ell in challenge ID: {}", parts[2]))
    })?;
    Ok((k, ell))
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

    loop {
        // Check shutdown
        if *shutdown.borrow() {
            info!("shutdown signal received, exiting");
            return Ok(());
        }

        // Fetch challenge info
        let challenge = client.get_challenge(&config.challenge_id).await?;
        let k = challenge.challenge.k;
        let ell = challenge.challenge.ell;
        let best_n = challenge.record.as_ref().map(|r| r.best_n);

        let target_n = match (config.start_n, best_n) {
            (Some(start), None) => start,
            (Some(start), Some(best)) if start > best => start,
            (_, Some(best)) => best + 1,
            (None, None) => 2, // Start small if no record exists
        };

        info!(
            challenge = %config.challenge_id,
            k, ell, ?best_n, target_n,
            "starting search round"
        );

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

            // Run search in blocking thread
            let n = target_n;
            let searcher = Arc::clone(searcher);
            let mut search_rng = SmallRng::from_rng(&mut rng).unwrap();
            let viz = viz_handle.clone();
            let result = tokio::task::spawn_blocking(move || {
                match viz {
                    Some(ref h) => {
                        let obs = VizObserver::new(Arc::clone(h));
                        searcher.search(n, k, ell, max_iters, &mut search_rng, &obs)
                    }
                    None => {
                        searcher.search(n, k, ell, max_iters, &mut search_rng, &NoOpObserver)
                    }
                }
            })
            .await
            .unwrap();

            let elapsed = start.elapsed();

            if result.valid {
                let (rarity, omega, alpha) =
                    compute_rarity(&result.graph, k, ell, false);
                info!(
                    strategy,
                    target_n,
                    iterations = result.iterations,
                    edges = result.graph.num_edges(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    ?rarity, omega, alpha,
                    "found valid graph!"
                );

                // Pin as valid discovery (not yet known if record)
                if let Some(ref vh) = viz_handle {
                    vh.pin_graph(
                        &result.graph, target_n, strategy, result.iterations,
                        false, rarity.clone(), omega, alpha,
                    );
                }

                // Encode and submit
                let rgxf_json = rgxf::to_json(&result.graph);
                match client.submit(&config.challenge_id, rgxf_json).await {
                    Ok(resp) => {
                        let is_record = resp.is_new_record.unwrap_or(false);
                        info!(
                            graph_cid = %resp.graph_cid,
                            verdict = %resp.verdict,
                            is_new_record = is_record,
                            "submitted graph"
                        );
                        if is_record {
                            info!("new record! n={target_n}");
                            if let Some(ref vh) = viz_handle {
                                let (rec_rarity, rec_omega, rec_alpha) =
                                    compute_rarity(&result.graph, k, ell, true);
                                vh.pin_graph(
                                    &result.graph, target_n, strategy, result.iterations,
                                    true, rec_rarity, rec_omega, rec_alpha,
                                );
                            }
                        }
                        consecutive_failures = 0;
                        found = true;
                        break;
                    }
                    Err(e) => {
                        error!("submit failed: {e}");
                        consecutive_failures += 1;
                    }
                }
            } else {
                warn!(
                    strategy,
                    target_n,
                    iterations = result.iterations,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "no valid graph found"
                );
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
    let (k, ell) = parse_challenge_params(&config.challenge_id)?;
    let target_n = config.start_n.unwrap_or(2);

    info!(
        challenge = %config.challenge_id,
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
            let result = tokio::task::spawn_blocking(move || {
                match viz {
                    Some(ref h) => {
                        let obs = VizObserver::new(Arc::clone(h));
                        searcher.search(n, k, ell, max_iters, &mut search_rng, &obs)
                    }
                    None => {
                        searcher.search(n, k, ell, max_iters, &mut search_rng, &NoOpObserver)
                    }
                }
            })
            .await
            .unwrap();

            let elapsed = start.elapsed();

            if result.valid {
                let (rarity, omega, alpha) =
                    compute_rarity(&result.graph, k, ell, false);
                info!(
                    strategy,
                    target_n,
                    round,
                    iterations = result.iterations,
                    edges = result.graph.num_edges(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    ?rarity, omega, alpha,
                    "found valid graph (offline)"
                );

                if let Some(ref vh) = viz_handle {
                    vh.pin_graph(
                        &result.graph, target_n, strategy, result.iterations,
                        false, rarity, omega, alpha,
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
