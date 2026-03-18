# MineGraph — Next Steps

Current state as of 2026-03-17.

## Where We Are

- **tree2 is the production default** with bitwise neighbor bitmask acceleration.
- **E005 completed:** 26.6h, 16 workers, 539 admits/hr, 96.5% admission rate.
  Leaderboard is NOT saturated — still room to grow.
- **Focused edge flipping implemented.** tree2 now supports `--focused` flag
  (default: true) which only mutates edges participating in violations.
  Based on Exoo & Tatarevic (2015) Algorithm 2.
- **E006 script ready.** Split fleet: 8 focused + 8 unfocused workers for
  head-to-head comparison.
- **Further gains require algorithmic changes, not hyperparameter tuning.**

## What to Do Next (RESUME HERE)

### Priority 0: Run E006 (Ready Now)

Overnight head-to-head: focused vs unfocused edge flipping.

```bash
# Prod server should already be running at localhost:3001
# From ~/RamseyNet-dev:
./scripts/experiment-e006.sh fleet

# Or headless:
nohup ./scripts/experiment-e006.sh fleet > /dev/null 2>&1 &

# Check progress:
cat logs/e006/status.txt
```

### Priority 1: Circulant Enumeration

Exhaustively check all 4,096 circulant graphs on 25 vertices (takes seconds).
Catalogues every valid R(5,5) circulant graph — algebraically structured with
high symmetry, good T3 scores, and superior seeds for local search.

### Priority 2: Cross-Round Tabu

Maintain a tabu list of recently-explored graph fingerprints across rounds via
`carry_state`. Prevents re-convergence on the same basins from different seeds.

### Priority 3: Score-Aware Valid-Space Walk

Once a valid graph is found, walk within the valid-graph space optimizing for
Goodman gap and symmetry. Distinct from finding more valid graphs.

### Priority 4: GPU Batch Evaluation

The bitwise inner loop maps trivially to GPU. RTX 4070 with 12GB available.
Only worth pursuing after software algorithmic improvements plateau.

## Completed

- [x] tree2 with bitwise NeighborSet acceleration
- [x] tree2 as default strategy
- [x] Fleet infrastructure with `--sweep` hyperparameter search
- [x] Experiment log system (`experiments/E001-E005.md`)
- [x] Experiment analysis tooling (`analyze_experiment.sh`)
- [x] MineGraph Gem renderer v3 (diamond matrix, web integration)
- [x] Server pipeline optimization (single transaction, no redundant nauty)
- [x] Cross-round state persistence
- [x] CI workflow (fmt + clippy + test + web build)
- [x] Literature review (`docs/LITERATURE_AND_IDEAS.md`)
- [x] Ed25519 signing system (key generation, submission signing, verification)
- [x] Metadata system (worker_id, commit_hash in submission metadata)
- [x] Focused edge flipping (guilty_edges + tree2 focused mode)
- [x] E005: 26.6h production fleet (539 admits/hr, 96.5% admission rate)
- [x] E006 experiment script (8 focused + 8 unfocused split fleet)

## Key Documents

| File | Purpose |
|------|---------|
| `NEXT_STEPS.md` | This file — current priorities and status |
| `experiments/E001-E005.md` | Experiment logs with results and analysis |
| `docs/LITERATURE_AND_IDEAS.md` | Paper summaries + strategy ideas (7-experiment roadmap) |
| `docs/STRATEGY_DEV_PLAN.md` | Strategy history, tree2 roadmap, experiment data |
| `docs/SIGNING_DESIGN.md` | Ed25519 identity system design |
| `docs/CLOUD_DEPLOYMENT.md` | Google Cloud Run deployment plan |

## Experiment Infrastructure

| Command | Purpose |
|---------|---------|
| `./scripts/experiment-e006.sh fleet` | Launch E006 split fleet (focused vs unfocused) |
| `./scripts/experiment-e005.sh fleet` | Launch E005 production fleet |
| `./run fleet` | Launch 16 workers (default: tree2, focused config) |
| `./run fleet --sweep` | Launch with hyperparameter sweep across profiles |
| `./run experiment` | Head-to-head strategy comparison (2 workers) |
| `./scripts/analyze_experiment.sh logs/fleet-*/` | Analyze completed experiment |
| `cat logs/e006/status.txt` | Check progress of E006 |

## Experiment Loop

The standard development cycle:

1. **Identify** the next algorithmic change (from `LITERATURE_AND_IDEAS.md` or brainstorming)
2. **Implement** the change in a new strategy or as a tree2 variant
3. **Run** `./run fleet --sweep` or dedicated experiment script against production server
4. **Analyze** with experiment status output or `./scripts/analyze_experiment.sh`
5. **Log** results in `experiments/ENNN.md`
6. **Decide** — promote the winner, identify next change, repeat
