# Extremal — Next Steps

Current state as of 2026-03-20.

## Where We Are

Extremal v1 is now the root project (moved from subproject into repo root).
All 11 backend crates implemented and working end-to-end. 62 tests passing.

- **tree2 is the production default** with bitwise neighbor bitmask acceleration.
- **Focused edge flipping implemented.** tree2 supports `--focused` flag
  (default: true) which only mutates edges participating in violations.
- **Full worker pipeline working:** engine loop with server client, leaderboard
  CID sync, biased seed sampling, Paley fallback for cold start.

## What to Do Next

### Priority 0: Canonical Labeling via nauty

CIDs are currently non-canonical — isomorphic graphs get different CIDs.
Wire up nauty C FFI for canonical labeling + automorphism group computation.

### Priority 1: Web Apps

Rebuild SvelteKit leaderboard + dashboard (skeleton in `web/`).

### Priority 2: Evo Strategy Port

Port the evolutionary SA strategy from the old prototype.

### Priority 3: Production Hardening

- Rate limiting
- Connection pool tuning
- Persistent server key management

### Priority 4: Circulant Enumeration

Exhaustively check all 4,096 circulant graphs on 25 vertices (takes seconds).
Catalogues every valid R(5,5) circulant graph — good seeds for local search.

### Priority 5: GPU Batch Evaluation

The bitwise inner loop maps trivially to GPU. RTX 4070 with 12GB available.
Only worth pursuing after software algorithmic improvements plateau.

## Completed

- [x] All 11 crates implemented (types, graph, scoring, identity, store, server, worker-api, strategies, worker-core, worker, cli)
- [x] tree2 with bitwise NeighborSet acceleration
- [x] Focused edge flipping (guilty_edges + tree2 focused mode)
- [x] Fleet infrastructure with `--sweep` hyperparameter search
- [x] Experiment log system (`experiments/E001-E006.md`)
- [x] Server pipeline (single transaction, SSE events, signed receipts)
- [x] Ed25519 signing system (key generation, submission signing, verification)
- [x] CI workflow (fmt + clippy + test + web build)
- [x] Migrated from subproject to root directory structure

## Key Documents

| File | Purpose |
|------|---------|
| `NEXT_STEPS.md` | This file — current priorities and status |
| `CLAUDE.md` | Architecture, API, scoring system, dev commands |
| `experiments/E001-E006.md` | Experiment logs with results and analysis |
| `docs/` | Legacy algorithm notes and literature review |

## Experiment Infrastructure

| Command | Purpose |
|---------|---------|
| `./run fleet` | Launch worker fleet |
| `./run worker` | Start single search worker |
| `./run server` | Start API server |
| `./scripts/fleet.sh` | Fleet launcher with options |
