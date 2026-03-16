# MineGraph — Next Steps

Current state as of 2026-03-16.

## Where We Are

- **tree2 is the production default** with bitwise neighbor bitmask acceleration.
- **Hyperparameter space is explored.** Four experiments (E001–E004) confirm that
  `focused` (beam=80, depth=12, bias=0.8) is the optimal config — 1.7x better
  than standard, and top-heavy leaderboard sampling (bias=0.8) is the dominant factor.
- **Leaderboard is NOT saturated.** 98.1% admission rate sustained over 7 hours
  at 1,034 admits/hr against 2000-slot board. Still room to grow.
- **Further gains require algorithmic changes, not hyperparameter tuning.**

## What to Do Next (RESUME HERE)

### Priority 1: Focused Edge Flipping

**The single highest-leverage algorithmic change available.**

From Exoo & Tatarevic (2015): only mutate edges that participate in violations
(monochromatic 5-cliques or 5-independent sets). Currently tree2 tries all 300
edges — but only ~20-50 are "guilty." Focusing mutations on those edges means
each mutation is **6-15x more likely to reduce violations.**

See `docs/LITERATURE_AND_IDEAS.md` section "HIGH PRIORITY — Implement Now, #1"
for full design.

### Priority 2: Circulant Enumeration

Exhaustively check all 4,096 circulant graphs on 25 vertices (takes seconds).
Catalogues every valid R(5,5) circulant graph — algebraically structured with
high symmetry, good T3 scores, and superior seeds for local search.

### Priority 3: Cross-Round Tabu

Maintain a tabu list of recently-explored graph fingerprints across rounds via
`carry_state`. Prevents re-convergence on the same basins from different seeds.

### Priority 4: Score-Aware Valid-Space Walk

Once a valid graph is found, walk within the valid-graph space optimizing for
Goodman gap and symmetry. Distinct from finding more valid graphs.

### Priority 5: GPU Batch Evaluation

The bitwise inner loop maps trivially to GPU. RTX 4070 with 12GB available.
Only worth pursuing after software algorithmic improvements plateau.

## Completed

- [x] tree2 with bitwise NeighborSet acceleration
- [x] tree2 as default strategy
- [x] Fleet infrastructure with `--sweep` hyperparameter search
- [x] Experiment log system (`experiments/E001-E004.md`)
- [x] Experiment analysis tooling (`analyze_experiment.sh`)
- [x] MineGraph Gem renderer v3 (diamond matrix, web integration)
- [x] Server pipeline optimization (single transaction, no redundant nauty)
- [x] Cross-round state persistence
- [x] CI workflow (fmt + clippy + test + web build)
- [x] Literature review (`docs/LITERATURE_AND_IDEAS.md`)

## Key Documents

| File | Purpose |
|------|---------|
| `NEXT_STEPS.md` | This file — current priorities and status |
| `experiments/E001-E004.md` | Experiment logs with results and analysis |
| `docs/LITERATURE_AND_IDEAS.md` | Paper summaries + strategy ideas (7-experiment roadmap) |
| `docs/STRATEGY_DEV_PLAN.md` | Strategy history, tree2 roadmap, experiment data |
| `docs/SIGNING_DESIGN.md` | Ed25519 identity system design (not yet built) |
| `docs/GPT_PROMPT.md` | External feedback prompt |

## Experiment Infrastructure

| Command | Purpose |
|---------|---------|
| `./run fleet` | Launch 16 workers (default: tree2, focused config) |
| `./run fleet --sweep` | Launch with hyperparameter sweep across profiles |
| `./run experiment` | Head-to-head strategy comparison (2 workers) |
| `./scripts/analyze_experiment.sh logs/fleet-*/` | Analyze completed experiment |
| `./scripts/render_gems.sh` | Render gem gallery from leaderboard |
| `cat logs/fleet-*/status.txt` | Check progress of running fleet |

## Experiment Loop

The standard development cycle:

1. **Identify** the next algorithmic change (from `LITERATURE_AND_IDEAS.md` or brainstorming)
2. **Implement** the change in a new strategy or as a tree2 variant
3. **Run** `./run fleet --sweep` or `./run experiment` against production server
4. **Analyze** with `./scripts/analyze_experiment.sh` — compare admits/hr and per-profile results
5. **Log** results in `experiments/ENNN.md`
6. **Decide** — promote the winner, identify next change, repeat
