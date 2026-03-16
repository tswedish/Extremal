# MineGraph

Distributed Ramsey graph search, competitive leaderboards, and deterministic generative graph art ("MineGraph Gems").

## What is this?

MineGraph searches for [Ramsey graphs](https://en.wikipedia.org/wiki/Ramsey%27s_theorem) — graphs that avoid monochromatic cliques and independent sets. The flagship target is R(5,5) on n=25 vertices, where 43 ≤ R(5,5) ≤ 48 is an open problem in combinatorics.

Workers search for candidate graphs, submit them to a central server for verification and scoring, and compete on leaderboards. Interesting graphs become visual artifacts — deterministic pixel-art "MineGraph Gems."

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+ with [pnpm](https://pnpm.io/)

### Build & Run

```bash
# Full CI: clippy + tests + web build
./run ci

# Start the API server
./run server --release

# In another terminal — start a search fleet
./run fleet --workers 16 --base-port 9000

# Or a single worker
./run search --release --k 5 --ell 5 --n 25 --port 8080
```

The server runs on `http://localhost:3001`. Worker dashboards on `http://localhost:9000` through `http://localhost:9015`.

### All Commands

```
./run ci          # Full CI: clippy + tests + web build
./run test        # Rust tests only
./run clippy      # Lint
./run build       # Build all crates
./run web         # Production web build
./run web-dev     # Web dev server (:5173)
./run server      # API server (:3001)
./run server-log  # API server with file logging
./run search      # Search worker (default: tree2, idle mode)
./run fleet       # Launch 16-worker fleet (production search)
./run fleet --sweep  # Fleet with hyperparameter sweep
./run experiment  # Head-to-head strategy comparison
./run bench       # Criterion benchmarks (verifier/scoring)
./run seed        # Seed test data
```

Add `--release` to `server`, `search`, `fleet`, `build`, `test` for optimized builds.

### Search Worker

```bash
./run search --k 5 --ell 5 --n 25                       # tree2 (default), default server
./run search --k 5 --ell 5 --n 25 --strategy tree       # original beam search
./run search --k 5 --ell 5 --n 25 --strategy evo        # evolutionary SA
./run search --k 3 --ell 4 --n 8 --server http://remote:3001 --max-iters 50000
./run search --k 4 --ell 4 --n 17 --offline --port 8080
```

Options: `--strategy {tree|tree2|evo|all}`, `--init {perturbed-paley|paley|random|leaderboard}`, `--noise-flips N`, `--max-iters N`, `--beam-width N`, `--max-depth N`, `--port PORT`, `--offline`, `--no-backoff`, `--sample-bias F`, `--leaderboard-sample-size N`, `--collector-capacity N`, `--max-known-cids N`.

## Experiment Loop

The development cycle for improving search strategies:

1. **Identify** the next algorithmic change (see `docs/LITERATURE_AND_IDEAS.md`)
2. **Implement** the change as a new strategy or tree2 variant
3. **Run** `./run fleet --sweep` or `./run experiment` against the production server
4. **Analyze** with `./scripts/analyze_experiment.sh logs/fleet-*/`
5. **Log** results in `experiments/ENNN.md`
6. **Decide** — promote the winner, identify next change, repeat

### Fleet Commands

```bash
# Production fleet (16 workers, best known config)
./run fleet --workers 16 --base-port 9000 \
  --beam-width 80 --max-depth 12 --sample-bias 0.8

# Hyperparameter sweep (6 profiles, auto-distributed)
./run fleet --sweep --base-port 9000

# Check progress without stopping
cat logs/fleet-*/status.txt

# Full analysis after stopping
./scripts/analyze_experiment.sh logs/fleet-*/
```

## Project Structure

```
crates/
  ramseynet-types/        Shared protocol types (GraphCid, RamseyParams, Verdict)
  ramseynet-graph/        RGXF graph encoding, neighbor bitmasks, SHA-256 CID
  ramseynet-verifier/     Ramsey verifier (clique detection, 4-tier scoring, automorphism)
  ramseynet-ledger/       SQLite ledger (submissions, leaderboards)
  ramseynet-server/       Axum HTTP server
  ramseynet-worker-api/   Search strategy trait + job/result schemas
  ramseynet-worker-core/  Worker engine: leaderboard sync, submission, init
  ramseynet-strategies/   Search strategy implementations (tree, tree2, evolutionary SA)
  ramseynet-worker/       CLI binary + worker web-app (visualization dashboard)
web/                      SvelteKit 2 / Svelte 5 frontend
scripts/                  Fleet, experiment, analysis, gem rendering scripts
experiments/              Experiment logs (E001–E004)
docs/                     Design docs, literature review, strategy roadmap
test-vectors/             Shared test data
```

## Leaderboard System

Every valid (K,L,n) triple defines a leaderboard of configurable capacity (default 500, production uses 2000). Submit directly with `{k, ell, n, graph}`.

**Scoring** (4-tier lexicographic, lower is better):
- **T1**: `(max(C_omega, C_alpha), min(C_omega, C_alpha))` — clique counts
- **T2**: Goodman gap — distance from theoretical minimum triangles
- **T3**: `|Aut(G)|` — automorphism group order (highest wins)
- **T4**: CID — deterministic tiebreaker

## Web Application

SvelteKit frontend with:
- **Homepage** — #1 gem showcase, server health badge
- **Leaderboards** — browse by (K,L) pairs, drill into ranked tables
- **Graph Visualization** — GemView (diamond matrix with hash-derived palette), MatrixView (adjacency matrix), CircleLayout (circle graph)
- **Submit** — paste RGXF JSON, live preview, submit for verification

## Server API

Port 3001, prefix `/api/`. SQLite at `./ramseynet.db`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check |
| `/api/leaderboards` | GET | List all (K,L,n) leaderboards with summary |
| `/api/leaderboards/{k}/{l}` | GET | List n values for a (K,L) pair |
| `/api/leaderboards/{k}/{l}/{n}` | GET | Paginated leaderboard (`?offset=0&limit=50`) + top graph |
| `/api/leaderboards/{k}/{l}/{n}/threshold` | GET | Admission threshold (score-to-beat) |
| `/api/leaderboards/{k}/{l}/{n}/graphs` | GET | RGXF for leaderboard entries (`?limit=N&offset=N`) |
| `/api/leaderboards/{k}/{l}/{n}/cids` | GET | Incremental CID sync (`?since=<ISO8601>`) |
| `/api/submissions/{cid}` | GET | Submission detail: graph, receipt, rank |
| `/api/verify` | POST | Stateless graph verification |
| `/api/submit` | POST | Full lifecycle: verify + store + leaderboard admit |

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier contract — JSON stdin/stdout, exit 0
- **Gem rendering**: `minegraph_gem_v3.py` (Python) and `GemView.svelte` (web component)

## Phase Status

| Phase | Status | Description |
|-------|--------|-------------|
| 0 — Scaffolding | Complete | Workspace, SvelteKit skeleton, CI |
| 1 — Graph Library | Complete | RGXF, AdjacencyMatrix, CID |
| 2 — Verifier | Complete | Clique detection, OVWC-1 |
| 3 — Server + Ledger | Complete | Axum API, SQLite |
| 4 — Web Application | Complete | Full interactive frontend with GemView |
| 5 — Search Worker | Complete | Tree/beam search, evolutionary SA |
| 5.5 — Leaderboard | Complete | 4-tier scoring, fleet infrastructure, experiment loop |
| 6 — Identity | Designed | Ed25519 signing (see `docs/SIGNING_DESIGN.md`) |

## License

MIT
