# RamseyNet

A permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

RamseyNet is a peer-to-peer network where anyone can propose and verify Ramsey graphs, persist artifacts in content-addressed storage, and derive public leaderboards and Pareto frontiers without central control.

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+ with [pnpm](https://pnpm.io/)

### Build & Run

```
# Full CI: clippy + tests + web build
./run ci

# Start the API server (with logging)
./run server-log

# In another terminal — start the web dev server
./run web-dev

# In a third terminal — seed test data
./run seed
```

The server runs on `http://localhost:3001` and the web app on `http://localhost:5173`.

See **[TESTING.md](TESTING.md)** for a full interactive walkthrough.

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
./run search      # Search worker (--k, --ell, --n required)
./run bench       # Criterion benchmarks (verifier/scoring)
./run seed        # Seed test data
```

### Search Worker

```
./run search --k 3 --ell 3 --n 5                     # all strategies, default server
./run search --k 3 --ell 3 --n 5 --strategy greedy
./run search --k 4 --ell 4 --n 17 --offline --port 8080  # no server needed
./run search --k 5 --ell 5 --n 25 --init leaderboard --sample-bias 0.3
```

Options: `--strategy {tree|all}`, `--init {perturbed-paley|paley|random|leaderboard}`, `--noise-flips N`, `--max-iters N`, `--beam-width N`, `--max-depth N`, `--port PORT`, `--offline`, `--no-backoff`, `--sample-bias F`, `--leaderboard-sample-size N`, `--collector-capacity N`.

## Project Structure

```
crates/
  ramseynet-types/        Shared protocol types (GraphCid, RamseyParams, Verdict)
  ramseynet-graph/        RGXF graph encoding + SHA-256 content addressing
  ramseynet-verifier/     Ramsey verifier (clique detection, 4-tier scoring, automorphism)
  ramseynet-ledger/       SQLite ledger (submissions, leaderboards, events)
  ramseynet-server/       Axum HTTP/WebSocket server
  ramseynet-worker-api/   Search strategy trait + job/result schemas
  ramseynet-worker-core/  Worker engine: leaderboard sync, submission, init
  ramseynet-strategies/   Search strategy implementations (tree/beam search)
  ramseynet-worker/       CLI binary + worker web-app (visualization)
web/                      SvelteKit 2 / Svelte 5 frontend (server web-app)
test-vectors/             Shared test data (small_graphs.json)
scripts/                  Dev helpers
```

## Leaderboard System

Every valid (K,L,n) triple implicitly defines a leaderboard of capacity 10,000 (configurable via `--leaderboard-capacity` on the server). No explicit "challenges" — submit directly with `{k, ell, n, graph}`. Capacity can be changed at server restart — shrinking trims the lowest-ranked entries automatically.

**Scoring** (4-tier lexicographic, lower is better):
- **T1**: `(max(C_omega, C_alpha), min(C_omega, C_alpha))` — clique counts, lowest wins
- **T2**: Goodman gap — distance from Goodman's minimum monochromatic triangles, lowest wins (0 = optimal)
- **T3**: `|Aut(G)|` — automorphism group order, highest wins
- **T4**: CID — deterministic tiebreaker, smallest wins

K ≤ L canonical form enforced everywhere (R(K,L) = R(L,K)).

## Web Application

The SvelteKit frontend provides:

- **Homepage** — Server health badge, navigation cards, live event feed
- **Leaderboards** (`/leaderboards`) — Browse by (K,L) pair, drill into n values
- **Leaderboard Detail** (`/leaderboards/[k]/[l]/[n]`) — Paginated ranked table with score columns (C_max, C_min, Goodman gap, |Aut|), top graph visualization, auto-refresh via polling, CSV export
- **Submission Detail** (`/submissions/[cid]`) — Full graph details: verdict, witness, rank, score breakdown (including Goodman number/gap/minimum), matrix + circle visualization
- **Submit** (`/submit`) — Enter K/L/N, paste RGXF JSON, see live matrix preview, submit for verification
- **Live Events** — Real-time OESP-1 WebSocket event stream with auto-reconnect

### Graph Visualization

- **MatrixView** — Canvas-rendered adjacency matrix with witness overlay (red highlights for violating cliques/independent sets)
- **CircleLayout** — SVG circle graph with deterministic vertex placement (ORS-1.0)

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
| `/api/events` | WS | OESP-1 event stream |

### Submit Request/Response

```json
// POST /api/submit
{ "k": 3, "ell": 3, "n": 5, "graph": { "n": 5, "encoding": "utri_b64_v1", "bits_b64": "..." } }
// Response
{ "graph_cid": "...", "verdict": "accepted", "admitted": true, "rank": 1, "score": {...} }
```

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier contract — JSON stdin/stdout, exit 0
- **OESP-1**: WebSocket event stream with monotonic sequence numbers

## Phase Status

| Phase | Status | Description |
|-------|--------|-------------|
| 0 — Scaffolding | Complete | Workspace, SvelteKit skeleton, CI |
| 1 — Graph Library | Complete | RGXF, AdjacencyMatrix, CID |
| 2 — Verifier | Complete | Clique detection, OVWC-1 WASM |
| 3 — Server + Ledger | Complete | Axum API, SQLite, WebSocket events |
| 4 — Web Application | Complete | Full interactive frontend |
| 5 — Search Worker | Complete | Greedy, local, SA, tree search |
| 5.5 — Leaderboard | Complete | 4-tier scoring (Goodman gap), 10k-cap leaderboards, pagination |
| 6 — P2P Networking | Pending | ed25519 identity, libp2p, duels |

## License

MIT
