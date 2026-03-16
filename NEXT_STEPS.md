# MineGraph — Next Steps

Current state as of 2026-03-16.

## Where We Are

- **tree2 with bitwise acceleration is deployed.** The inner loop now uses
  `u64` neighbor bitmasks for clique counting — AND/popcount instead of
  per-vertex `edge()` calls. Expected 5-10x speedup over previous tree2.
- **tree2 is the default strategy** (`--strategy` defaults to `tree2`).
- **16-worker fleet** infrastructure with `./run fleet` and `--sweep` mode
  for hyperparameter search across beam_width, max_depth, sample_bias.
- **First experiment completed** (tree vs tree2 pre-bitwise): tree2 was 11x
  faster and got 4.6x more admissions. Pre-bitwise tree2 plateau'd after
  extended runs on the 500-slot board.
- **Fleet run completed** (16 x tree2 pre-bitwise): 136M discoveries, 10.8K
  admissions, 93.4% admission rate against 2000-slot board.

## What to Do Next (RESUME HERE)

### Immediate: Run bitwise fleet experiment

The bitwise implementation just landed. Run a fleet to measure the actual
speedup vs the pre-bitwise baseline:

```bash
# Start server (if not running)
./run server --release --leaderboard-capacity 2000

# Run bitwise fleet
./run fleet --sweep --base-port 9000
```

Let it run 15-30 minutes, then compare:
- **Round time** — expect ~30-80ms avg (vs ~330ms pre-bitwise)
- **Total discoveries** — expect 5-10x more in same wall time
- **Admission rate** — may drop if leaderboard is already well-populated
- **Per-profile results** — which hyperparameter combo is best?

### Priority 2: Diversity-Aware Beam Selection

Once the leaderboard saturates, finding *more* valid graphs doesn't help — we need
*better-scoring* graphs or graphs in unexplored regions. Options:

- Add a novelty bonus to beam selection (penalize candidates similar to existing beam members)
- Maintain a fingerprint archive across rounds
- Use graph invariants (degree sequence, triangle count) as diversity signals

### Priority 3: Score-Aware Search

Current search optimizes violation count (reach 0 = valid). Once valid, all graphs
are treated equally. To improve leaderboard rank, the search should:

- Among valid candidates in the beam, prefer those with better Goodman gap
- Use automorphism-group-order proxy during search (expensive, but only for valid candidates)

### Priority 4: GPU Batch Evaluation (after bitwise proves out)

The bitwise inner loop (AND/popcount, no branching) now maps trivially to GPU:
- Each CUDA thread processes one (parent, edge) pair
- Zero warp divergence since all threads do identical bit ops
- RTX 4070 available with 12GB VRAM, currently unused
- Expected additional 10-50x on top of bitwise CPU

## Completed

- [x] Bitwise adjacency operations (NeighborSet, violation_delta_bitwise)
- [x] tree2 default strategy
- [x] Fleet with --sweep hyperparameter search
- [x] Experiment analysis tooling (analyze_experiment.sh)
- [x] MineGraph Gem renderer v3 (diamond matrix style)
- [x] Server submission pipeline optimization (single transaction, no redundant nauty)
- [x] Cross-round state persistence for strategies

## Experiment Infrastructure

- `./run experiment` — head-to-head strategy comparison
- `./run fleet` — launch N workers (uniform or `--sweep`)
- `./scripts/analyze_experiment.sh` — compact analysis of experiment logs
- `./scripts/render_gems.sh` — render gems from leaderboard
- Logs in `logs/experiment-*/` and `logs/fleet-*/`
