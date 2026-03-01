# RamseyNet

Permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

## Quick Start

```
./run ci          # Full CI: clippy + tests + web build
./run test        # Rust tests only
./run server      # API server on :3001
./run server-log  # API server with file logging
./run web-dev     # SvelteKit dev server on :5173
./run search      # Search worker (requires --challenge; see below)
./run seed        # Seed DB with test data
```

Other commands: `clippy`, `build`, `web` (production build).

### Search Worker

```
./run search --challenge ramsey:3:3:v1          # all strategies, default server
./run search --challenge ramsey:3:3:v1 --strategy greedy --start-n 4
./run search --challenge ramsey:3:4:v1 --server http://remote:3001 --max-iters 50000
./run search --challenge ramsey:3:3:v1 --offline --start-n 5 --viz-port 8080  # no server needed
```

Options: `--strategy {greedy|local|annealing|all}`, `--start-n N`, `--max-iters N`, `--tabu-tenure N`, `--initial-temp F`, `--cooling-rate F`, `--viz-port PORT`, `--offline`, `--no-backoff`.

## Architecture

Rust workspace (`crates/`) + SvelteKit 2 (`web/`).

**Crate dependency order:** `types` → `graph` → `verifier` → `ledger` → `server` ← `search`

| Crate | Purpose |
|-------|---------|
| `ramseynet-types` | Shared newtypes: GraphCid, ChallengeId, RamseyParams, Verdict |
| `ramseynet-graph` | RGXF encode/decode, AdjacencyMatrix, SHA-256 CID |
| `ramseynet-verifier` | Clique/independent-set detection, OVWC-1 WASM binary |
| `ramseynet-ledger` | SQLite ledger: challenges, submissions, receipts, records, events |
| `ramseynet-server` | Axum REST + WebSocket, full submit lifecycle |
| `ramseynet-search` | Standalone CLI: greedy, local search, simulated annealing, worker loop |

## Web App (SvelteKit 2 / Svelte 5)

**Components** in `web/src/lib/components/`:

| Component | Purpose |
|-----------|---------|
| `MatrixView` | Canvas adjacency matrix with witness overlay |
| `CircleLayout` | SVG circle graph (ORS-1.0) |
| `EventFeed` | Live OESP-1 event ticker with CID links |
| `SubmitForm` | RGXF paste + preview + submit |

**Stores/utils:** `events.svelte.ts` (WebSocket + auto-reconnect), `rgxf.ts` (client-side RGXF decoder), `api.ts` (typed fetch wrappers).

**Routes:**

| Route | Purpose |
|-------|---------|
| `/` | Homepage with health badge, nav cards, live event feed |
| `/challenges` | Challenge list with best-known records |
| `/challenges/[id]` | Challenge detail + graph viz + inline submit |
| `/records` | Best-known records table |
| `/submissions/[cid]` | Submission detail: verdict, witness, graph viz |
| `/submit` | Standalone graph submission form |

## Server API

Port 3001, prefix `/api/`. SQLite at `./ramseynet.db`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check |
| `/api/challenges` | GET/POST | List or create challenges |
| `/api/challenges/{id}` | GET | Challenge detail + current record + record_graph RGXF |
| `/api/records` | GET | Best-known records |
| `/api/submissions/{cid}` | GET | Submission detail: graph, receipt, challenge context |
| `/api/verify` | POST | Stateless graph verification |
| `/api/submit` | POST | Full lifecycle: verify + store + record update |
| `/api/events` | WS | OESP-1 event stream |

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier contract — JSON stdin/stdout, exit 0
- **OESP-1**: WebSocket event stream with monotonic sequence numbers

## Test Data

- `test-vectors/small_graphs.json` — C5, K5, E5, Petersen, Wagner with RGXF encodings
- `scripts/seed-ledger.sh` — creates challenges and submits test graphs via the API

## Phase Status

Phases 0–5 complete. Phase 6 (ed25519 identity, duels, libp2p) is next.
