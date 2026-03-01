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
use crate::viz::{NoOpObserver, VizHandle, VizObserver};

/// Configuration for the worker loop.
pub struct WorkerConfig {
    pub challenge_id: String,
    pub start_n: Option<u32>,
    pub max_iters: u64,
}

/// Run the search worker loop.
pub async fn run_worker(
    client: ServerClient,
    searchers: Vec<Box<dyn Searcher>>,
    config: WorkerConfig,
    mut shutdown: watch::Receiver<bool>,
    viz_handle: Option<Arc<VizHandle>>,
) -> Result<(), SearchError> {
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
                info!(
                    strategy,
                    target_n,
                    iterations = result.iterations,
                    edges = result.graph.num_edges(),
                    elapsed_ms = elapsed.as_millis() as u64,
                    "found valid graph!"
                );

                // Pin as valid discovery (not yet known if record)
                if let Some(ref vh) = viz_handle {
                    vh.pin_graph(&result.graph, target_n, strategy, result.iterations, false);
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
                                vh.pin_graph(&result.graph, target_n, strategy, result.iterations, true);
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
