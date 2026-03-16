# Strategy Development Plan

Living document for developing new search strategies, running them against prod,
and evolving toward agentic strategy iteration.

**Status:** Phase 1 — Tree2 winning 4.6x over tree1 on R(5,5) n=25. Iterating.

---

## Goal

Build new search strategies in this worktree, run a local worker against the
production server, and compete on the leaderboard. Eventually, make the
develop-test-iterate loop agentic so Claude can search over strategy variants
autonomously and converge on better approaches.

## Current Baseline

The only strategy is **TreeSearch** (beam search over single-edge flips):
- Maintains a beam of `beam_width` candidates (default 100)
- At each depth, tries all single-edge mutations per candidate
- Keeps the `beam_width` lowest-violation children
- Scores via `count_cliques(G, k) + count_cliques(complement, ell)` (violation count)
- Valid graphs (violation = 0) are streamed via `observer.on_discovery()`
- CID-based dedup prevents re-exploring seen graphs

**Strengths:** Systematic, discovers many valid graphs per run, good for small n.
**Weaknesses:** Single-edge mutations only, no exploitation of graph structure,
no adaptive mutation rate, no scoring feedback during search (only violation
counts, not the 4-tier `GraphScore`), expensive for large n (tries all n(n-1)/2
edges per parent).

## Architecture Constraints

New strategies must implement the `SearchStrategy` trait in `ramseynet-worker-api`:

```rust
pub trait SearchStrategy: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn config_schema(&self) -> Vec<ConfigParam>;
    fn search(&self, job: &SearchJob, observer: &dyn SearchObserver) -> SearchResult;
}
```

Key design rules:
- **Pure computation.** No network, no filesystem, no async. Everything comes
  in via `SearchJob`, results go out via `SearchResult` and `observer`.
- **Stateless across rounds.** The engine calls `search()` once per round with
  a fresh `SearchJob`. Cross-round state lives in the engine (local pool, CIDs).
- **Cancellable.** Must check `observer.is_cancelled()` periodically.
- **CID dedup.** `job.known_cids` contains server-known CIDs; strategies should
  use this to avoid re-exploring already-submitted graphs.
- **Config via JSON.** Strategy-specific params come from `job.config` (a
  `serde_json::Value`). Expose them via `config_schema()` for the dashboard UI.

Registration: add to `default_strategies()` in `crates/ramseynet-strategies/src/lib.rs`.

## What the Worker Infrastructure Provides

- **Seed graphs:** Paley, perturbed-Paley, random, or sampled from the server
  leaderboard (with noise flips). Arrives as `job.init_graph`.
- **Known CIDs:** Incrementally synced from the server. Passed as `job.known_cids`.
- **Scoring + canonicalization:** After each round, the engine calls
  `compute_score_canonical()` on all discoveries (nauty canonical form, clique
  counting, Goodman gap, automorphism order).
- **Submission:** Engine handles threshold checks and `POST /api/submit`.
- **Local pool:** Self-learning buffer of best discoveries, used as seed pool
  in subsequent rounds (unless in Leaderboard init mode).
- **Dashboard:** WebSocket viz at `--port PORT` with real-time snapshots,
  leaderboard, and strategy controls.

---

## Phase 1: EvoSearch — Implemented

### What was built

**EvoSearch** (`crates/ramseynet-strategies/src/evo.rs`): Evolutionary simulated
annealing with a small population.

**Core algorithm:**
- Population of `pop_size` (default 4) individuals, each running SA independently
- Round-robin iteration: each step, one individual flips a random edge
- **Incremental violation counting:** Only recomputes cliques through the
  flipped edge pair, not the full graph. For R(5,5) n=25: C(23,3) = 1,771
  subsets per color vs C(25,5) = 53,130 for full recount. ~30x speedup.
- **SA acceptance:** Always accept improvements; accept worse moves with
  probability exp(-delta / temperature). Exponential cooling schedule.
- **Crossover:** Periodically, best individual's vertex neighborhood is copied
  to the worst individual (row-swap crossover).
- **Stale restart:** If an individual hasn't improved for N iterations, it's
  re-initialized with heavy perturbation.
- **Periodic full recount:** Every 10K iterations per individual, full violation
  recount to correct any drift from incremental computation.

**Config params:**
- `pop_size` (1-32, default 4) — population size
- `temp_start` (0.01-100, default 2.0) — initial SA temperature
- `temp_end` (0.001-10, default 0.01) — final SA temperature
- `crossover_interval` (0-1M, default 5000) — iters between crossover events
- `restart_stale` (0-1M, default 50000) — restart after N iters without improvement

**Cross-round persistence:** Population is carried across rounds via
`SearchResult::carry_state` / `SearchJob::carry_state` (new API). The
population evolves continuously across server sync boundaries.

**Dashboard observability:** Reports best individual's violation score, clique
counts, and discovery count every 500 iterations.

### Infrastructure changes

- `SearchJob` and `SearchResult` now have `carry_state: Option<Box<dyn Any + Send>>`
  for opaque cross-round state persistence
- Engine stores `strategy_state` and threads it through the round loop
- `strategy_state` is cleared on `Start` command (fresh search)

### What's next (ideas for iteration)

- **Structured mutations:** vertex swaps, row perturbations, block mutations
- **Score-aware search for valid graphs:** once violations reach 0, use
  triangle count as Goodman gap proxy to guide toward better-scoring graphs
- **Adaptive temperature:** adjust based on acceptance rate rather than fixed schedule
- **Multi-flip mutations:** flip k edges at once, with k adaptive to plateau detection

---

## Tree2: Incremental Beam Search — Implemented

### What was built

**Tree2Search** (`crates/ramseynet-strategies/src/tree2.rs`): Same beam search
skeleton as tree1, but with three key optimizations making each candidate
evaluation ~10-30x cheaper.

**Key differences from tree1:**

| Aspect | tree1 | tree2 |
|--------|-------|-------|
| Candidate eval | Clone parent + full `count_cliques` x2 + complement | Flip-in-place + incremental delta + unflip |
| Dedup | SHA-256 CID (~200ns) | 64-bit XOR-fold fingerprint (~5ns) |
| Complement | Rebuilt from scratch per candidate | Carried per beam entry, maintained incrementally |
| Allocations per candidate | 1 clone + 1 complement | 0 (flip-score-unflip in place) |
| Full CID computation | Every candidate | Only valid discoveries |
| Full recount | Never (trusts incremental) | Once per beam entry when materializing new beam |

**Shared infrastructure:** Incremental violation counting functions extracted
into `crates/ramseynet-strategies/src/incremental.rs`, shared between evo and
tree2: `violation_delta`, `count_cliques_through_edge`,
`count_cliques_through_edge_assuming`, `fast_fingerprint`.

**Debug logging:** Per-depth-level summary via `tracing::debug!`:
```
tree2: depth complete depth=3 beam_size=100 candidates=28500 dedup_hits=1200
  best_score=2 worst_score=15 discoveries=0 seen_set=85000 elapsed_ms=340
```
Visible with `RUST_LOG=ramseynet_strategies=debug` or `./run search -v`.

### Tree2 Improvement Roadmap

| Version | Change | Expected Impact |
|---------|--------|----------------|
| **v0** (done) | Incremental delta + flip-score-unflip + cheap fingerprint + complement per beam entry | ~10-30x faster per candidate eval |
| **v1** | Diversity-aware beam selection (keep structurally diverse candidates, not just lowest score) | Better exploration of different graph basins |
| **v2** | Multi-flip mutations (flip 2-3 edges per candidate) | Escape local minima, deeper search per depth |
| **v3** | Adaptive beam width (widen on score plateaus, narrow when one dominates) | Better resource allocation |
| **v4** | Carry state across rounds (persist beam as population) | Continuous improvement like evo |
| **v5** | Hybrid: beam search to find valid graphs, then SA refinement within valid space | Optimize score tiers, not just find valid graphs |

### Experiment 1 Results: tree vs tree2 (2026-03-15)

Duration ~4.5 hours, R(5,5) n=25, both `--init leaderboard`, release mode.

| Metric | tree | tree2 | Ratio |
|--------|------|-------|-------|
| Rounds completed | 373 | 2,109 | **5.7x** |
| Round time | ~5,000ms | ~440ms | **11x faster** |
| Total discoveries | 1.5M | 8.7M | **5.7x** |
| Leaderboard admits | 1,051 | 4,885 | **4.6x** |
| Admission rate | 54% | 76% | tree2 higher |
| Discoveries/round | ~4,080 | ~3,920 | ~same |

**Conclusion:** Tree2 v0 is a decisive improvement. The incremental delta approach
delivers ~11x faster rounds and 4.6x more leaderboard admissions. The higher
admission rate (76% vs 54%) suggests tree2 finds more diverse graphs, not just
more of the same. Next priority: score optimization and diversity-aware beam
selection, since the leaderboard threshold is now high enough that finding
*any* valid graph is less useful than finding *better-scoring* ones.

---

## Competitive Iteration Framework

The core development loop: iteratively build better strategies by pitting them
against each other at the current frontier. Each experiment produces a "loss
gradient" — concrete data on what works and what doesn't, which informs the
next strategy modification.

### Methodology

1. **Run two workers** (strategy A vs B) against the same server/leaderboard
2. **Collect structured logs** (`round_summary` lines) over 30-60 minutes
3. **Compare key metrics**: discoveries/min, admissions/min, best score
4. **Analyze** what the winning strategy does differently
5. **Modify** the losing strategy (or create a new variant)
6. **Repeat**

### Key Metrics (from `round_summary` log lines)

| Metric | What it means | How to compute |
|--------|--------------|----------------|
| **Discoveries/min** | Valid graphs found per wall-minute | `total_discoveries / elapsed_minutes` |
| **Admissions/min** | Graphs admitted to leaderboard per wall-minute | `total_admitted / elapsed_minutes` |
| **Admission rate** | Fraction of submissions that get admitted | `total_admitted / total_submitted` |
| **Round time** | How long each search round takes | `elapsed_ms` per round_summary |
| **Depth efficiency** (tree2) | Per-depth-level stats from debug logs | grep `tree2: depth complete` |

### Analyzing Experiment Logs

```bash
# Extract round summaries for a specific strategy
grep 'round_summary' logs/worker-tree-*.log

# Count total admissions
grep -c 'admitted to leaderboard' logs/worker-tree-*.log

# Compare discoveries/min between two runs
grep 'round_summary' logs/worker-tree-*.log | tail -1
grep 'round_summary' logs/worker-tree2-*.log | tail -1

# Tree2 depth-level analysis (run worker with -v)
grep 'tree2: depth complete' logs/worker-tree2-*.log
```

### Leaderboard Sizing Decisions

| Leaderboard Size | When to use | Effect on competition |
|-----------------|-------------|----------------------|
| **50** | Early development, quick feedback | Threshold rises fast; only the best graphs survive. Good for testing score optimization. |
| **500** (current default) | Standard competition | Healthy balance — enough room for diversity, but threshold is meaningful. |
| **5,000+** | Populating a new (k,ell,n) target | Easy admission, good for collecting diverse valid graphs. Tighten later. |

**When to grow:** If admission rate drops below ~5% for both strategies, the
leaderboard is saturated and neither strategy is improving. Options:
- Increase leaderboard capacity (more room at the bottom)
- Switch to a harder target (larger n)
- Focus on score optimization rather than finding new valid graphs

**When to shrink:** If the leaderboard has many low-quality entries from the
initial flood, shrink it to force strategies to compete on quality. The server
trims automatically on restart with `--leaderboard-capacity N`.

### Running an Experiment

```bash
# 1. Start the server (release mode, port 3002)
./run server --release --port 3002

# 2. Start worker A (tree, logging to file)
./run search --release --strategy tree --k 5 --ell 5 --n 25 \
  --server http://localhost:3002 --init leaderboard --port 8080 \
  2>&1 | tee logs/worker-tree-$(date +%Y%m%d-%H%M%S).log &

# 3. Start worker B (tree2, logging to file)
./run search --release --strategy tree2 --k 5 --ell 5 --n 25 \
  --server http://localhost:3002 --init leaderboard --port 8081 \
  2>&1 | tee logs/worker-tree2-$(date +%Y%m%d-%H%M%S).log &

# 4. Watch dashboards
#    tree:  http://localhost:8080
#    tree2: http://localhost:8081

# 5. After 30-60 min, Ctrl+C both workers and analyze:
grep 'round_summary' logs/worker-tree-*.log | tail -5
grep 'round_summary' logs/worker-tree2-*.log | tail -5
```

---

## Phase 2: Evaluation Harness

Build tooling to compare strategies objectively before running against prod.

### 2a. Offline Benchmark

A Rust binary (or test) that:
1. Runs each strategy on a fixed set of (k, ell, n) targets with fixed seeds
2. Measures: time to first valid graph, number of valid graphs found, best
   `GraphScore` achieved, iterations used
3. Outputs a comparison table

This uses `--offline` mode — no server needed.

### 2b. Score Tracking

Track best scores achieved per (k, ell, n) per strategy over time. Store in a
local JSON/SQLite file. Compare against the prod leaderboard threshold to
predict admission likelihood before submitting.

### 2c. Dashboard Enhancements

- Add per-strategy comparison view (score over time, discovery rate)
- Add strategy-switching without stopping (preserve local pool across restarts)

---

## Phase 3: Agentic Strategy Iteration

Make the develop-test-iterate loop autonomous.

### 3a. Strategy Parameterization

Expose all strategy hyperparameters as a structured search space. Each strategy
variant is a point in this space (e.g., beam_width=200, max_depth=15,
accept_equal=true, temp_start=0.5).

### 3b. Evaluation Oracle

A function that takes a strategy config, runs it for N rounds on a target
(k, ell, n), and returns a performance metric:
- Primary: best `GraphScore` achieved (4-tier comparison against current
  leaderboard threshold)
- Secondary: discovery rate (valid graphs per wall-second)
- Tertiary: admission rate (graphs that beat the threshold)

### 3c. Agentic Loop

Claude operates the outer loop:
1. **Observe:** Read current leaderboard state, threshold, and past experiment
   results from the score tracking DB.
2. **Hypothesize:** Propose a strategy variant (new params, new mutation
   operator, new algorithm) based on what has/hasn't worked.
3. **Test:** Run the variant via the evaluation oracle (offline first, then
   online against prod).
4. **Analyze:** Compare results against baseline and previous variants.
5. **Iterate:** Refine the hypothesis and repeat.

This requires:
- A persistent experiment log (what was tried, what happened)
- A way to run strategies programmatically (not just via CLI)
- Analysis of leaderboard structure (what score tiers are competitive?)

### 3d. Strategy Code Generation

The most ambitious step: Claude generates new strategy implementations in Rust,
compiles them, runs the evaluation oracle, and iterates on the code. This
requires:
- A template for new strategies (implement `SearchStrategy`, register, compile)
- A fast compile-test cycle (incremental compilation + targeted test)
- Safety rails (compilation errors are feedback, not failures)

---

## Phase 4: Production Deployment

### 4a. Multi-Strategy Worker

Run a worker that cycles through strategies (or runs the best-performing one)
against the production server.

```bash
./run search --k 4 --ell 4 --n 17 --strategy all --server https://prod:3001
```

### 4b. Leaderboard Monitoring

Track our admission rate and rank distribution over time. Detect when the
leaderboard becomes saturated (threshold stops moving) and switch to more
explorative strategies.

### 4c. Multi-Target Optimization

Run workers on multiple (k, ell, n) targets simultaneously, allocating compute
to the targets where we're most likely to achieve admissions.

---

## Implementation Order

| Step | Description | Depends On |
|------|-------------|------------|
| **1** | Implement greedy random walk strategy (1a) | — |
| **2** | Run against prod, verify admission pipeline works | 1 |
| **3** | Implement multi-flip mutation (1b) | 1 |
| **4** | Build offline benchmark harness (2a) | 1, 3 |
| **5** | Implement score-aware beam search (1c) | 1 |
| **6** | Implement hybrid beam+SA (1d) | 1, 5 |
| **7** | Build score tracking DB (2b) | 4 |
| **8** | Implement agentic evaluation oracle (3b) | 4, 7 |
| **9** | Implement agentic loop (3c) | 8 |
| **10** | Strategy code generation (3d) | 9 |

---

## Open Questions

1. **Which (k, ell, n) targets should we focus on?** Small n (5-10) for fast
   iteration, or larger n (17+) where the leaderboard is more competitive?
2. **Should strategies use the 4-tier GraphScore during search?** Currently only
   violation counts are used. Computing full scores is expensive (nauty) but
   would allow optimizing for Goodman gap and symmetry during search, not just
   post-hoc.
3. **Can we share state across rounds?** The current architecture clears
   strategy state between rounds. Some algorithms (e.g., evolutionary
   strategies, population-based methods) benefit from persistent populations.
4. **What's the compile-test cycle time?** For agentic code generation, we need
   fast iteration. Incremental compilation of just `ramseynet-strategies` should
   be fast, but we should measure.

---

## References

- Strategy trait: `crates/ramseynet-worker-api/src/strategy.rs`
- Current strategies: `crates/ramseynet-strategies/src/`
- Engine loop: `crates/ramseynet-worker-core/src/engine.rs`
- Scoring: `crates/ramseynet-verifier/src/scoring.rs`
- Init modes: `crates/ramseynet-worker-core/src/init.rs`
- Worker CLI: `crates/ramseynet-worker/src/main.rs`
- Dashboard viz: `crates/ramseynet-worker/src/viz/`
