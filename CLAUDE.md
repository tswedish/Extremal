# RamseyNet

Permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

## Quick Start

```
./run ci          # Full CI: clippy + tests + web build
./run test        # Rust tests only
./run server      # API server on :3001
./run server-log  # API server with file logging
./run web-dev     # SvelteKit dev server on :5173
./run search      # Search worker (requires --k, --ell, --n; see below)
./run seed        # Seed DB with test data
```

Other commands: `clippy`, `build`, `web` (production build).

### Search Worker

```
./run search --k 3 --ell 3 --n 5                     # all strategies, default server
./run search --k 3 --ell 3 --n 5 --strategy greedy
./run search --k 3 --ell 4 --n 8 --server http://remote:3001 --max-iters 50000
./run search --k 4 --ell 4 --n 17 --offline --viz-port 8080  # no server needed
```

Options: `--strategy {greedy|local|annealing|tree|all}`, `--init {perturbed-paley|paley|random|balanced|leaderboard}`, `--noise-flips N`, `--max-iters N`, `--tabu-tenure N`, `--initial-temp F`, `--cooling-rate F`, `--beam-width N`, `--max-depth N`, `--viz-port PORT`, `--offline`, `--no-backoff`.

**Discovery submission:** All valid graphs found during search (not just the final result) are collected in a bounded, score-sorted buffer (top 100) and submitted to the server. This is especially useful for tree/beam search which discovers many valid graphs per run.

## Architecture

Rust workspace (`crates/`) + SvelteKit 2 (`web/`).

**Crate dependency order:** `types` → `graph` → `verifier` → `ledger` → `server` ← `search`

| Crate | Purpose |
|-------|---------|
| `ramseynet-types` | Shared newtypes: GraphCid, RamseyParams, Verdict |
| `ramseynet-graph` | RGXF encode/decode, AdjacencyMatrix, SHA-256 CID |
| `ramseynet-verifier` | Clique/independent-set detection, 3-tier scoring, automorphism |
| `ramseynet-ledger` | SQLite ledger: submissions, receipts, leaderboards, events |
| `ramseynet-server` | Axum REST API, full submit lifecycle |
| `ramseynet-search` | Standalone CLI: greedy, local search, simulated annealing, tree search |

## Leaderboard System

Every valid (K,L,n) triple implicitly defines a leaderboard of capacity 100. No explicit "challenges" — submit directly with `{k, ell, n, graph}`.

**Scoring** (3-tier lexicographic, lower is better):
- **T1**: `(max(C_omega, C_alpha), min(C_omega, C_alpha))` — clique counts, lowest wins
- **T2**: `|Aut(G)|` — automorphism group order, highest wins
- **T3**: CID — deterministic tiebreaker, smallest wins

K ≤ L canonical form enforced everywhere (R(K,L) = R(L,K)).

## Web App (SvelteKit 2 / Svelte 5)

**Components** in `web/src/lib/components/`:

| Component | Purpose |
|-----------|---------|
| `MatrixView` | Canvas adjacency matrix with witness overlay |
| `CircleLayout` | SVG circle graph (ORS-1.0) |
| `SubmitForm` | K/L/N inputs + RGXF paste + preview + submit |

**Utils:** `rgxf.ts` (client-side RGXF decoder), `api.ts` (typed fetch wrappers).

**Routes:**

| Route | Purpose |
|-------|---------|
| `/` | Homepage with health badge, nav cards, live event feed |
| `/leaderboards` | Browse by (K,L) pairs, drill into n values |
| `/leaderboards/[k]/[l]/[n]` | Ranked table with score columns, top graph viz, auto-refresh via polling |
| `/submissions/[cid]` | Submission detail: verdict, witness, graph viz, rank |
| `/submit` | Standalone graph submission form |

## Server API

Port 3001, prefix `/api/`. SQLite at `./ramseynet.db`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check |
| `/api/leaderboards` | GET | List all (K,L,n) leaderboards with summary |
| `/api/leaderboards/{k}/{l}` | GET | List n values for a (K,L) pair |
| `/api/leaderboards/{k}/{l}/{n}` | GET | Full leaderboard with entries + top graph |
| `/api/leaderboards/{k}/{l}/{n}/threshold` | GET | Admission threshold (score-to-beat) |
| `/api/leaderboards/{k}/{l}/{n}/graphs` | GET | RGXF for top leaderboard entries (`?limit=N`) |
| `/api/submissions/{cid}` | GET | Submission detail: graph, receipt, rank |
| `/api/verify` | POST | Stateless graph verification |
| `/api/submit` | POST | Full lifecycle: verify + store + leaderboard admit |
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

## Test Data

- `test-vectors/small_graphs.json` — C5, K5, E5, Petersen, Wagner with RGXF encodings
- `scripts/seed-ledger.sh` — submits test graphs via the API (no challenge creation)

## Phase Status

Phases 0–5 complete. Phase 6 (ed25519 identity, duels, libp2p) is next.
