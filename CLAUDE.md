# RamseyNet

Permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

## Build & Test

Dev environment: WSL2 Ubuntu 24.04. Helper script: `scripts/wsl-dev.sh`.

```bash
# Native WSL2 (recommended — run from repo root)
bash scripts/wsl-dev.sh ci          # Full CI: clippy + tests + web build
bash scripts/wsl-dev.sh test        # cargo test --all
bash scripts/wsl-dev.sh clippy      # cargo clippy
bash scripts/wsl-dev.sh web         # pnpm install && pnpm build
bash scripts/wsl-dev.sh web-dev     # pnpm dev (live reload on :5173)
bash scripts/wsl-dev.sh server      # API server on :3001
bash scripts/wsl-dev.sh server-log  # API server with file logging (logs/)
bash scripts/wsl-dev.sh seed        # Seed DB with test challenges + graphs

# From Windows PowerShell (alternative)
wsl.exe -d Ubuntu -e bash scripts/wsl-dev.sh ci
```

## Architecture

Rust workspace (`crates/`) + SvelteKit 2 (`web/`).

**Crate order:** `types` → `graph` → `verifier` → `ledger` → `server` ← `search`

| Crate | Purpose |
|-------|---------|
| `ramseynet-types` | Shared newtypes: GraphCid, ChallengeId, RamseyParams, Verdict |
| `ramseynet-graph` | RGXF encode/decode, AdjacencyMatrix, SHA-256 CID |
| `ramseynet-verifier` | Clique/independent-set detection, OVWC-1 WASM binary |
| `ramseynet-ledger` | SQLite ledger: challenges, submissions, receipts, records, events |
| `ramseynet-server` | Axum REST + WebSocket, full submit lifecycle |
| `ramseynet-search` | Greedy, local search, simulated annealing, worker loop |

## Web App (SvelteKit 2 / Svelte 5)

| Component | File | Purpose |
|-----------|------|---------|
| `MatrixView` | `web/src/lib/components/MatrixView.svelte` | Canvas adjacency matrix with witness overlay |
| `CircleLayout` | `web/src/lib/components/CircleLayout.svelte` | SVG circle graph (ORS-1.0) |
| `EventFeed` | `web/src/lib/components/EventFeed.svelte` | Live OESP-1 event ticker |
| `SubmitForm` | `web/src/lib/components/SubmitForm.svelte` | RGXF paste + preview + submit |
| `rgxf.ts` | `web/src/lib/rgxf.ts` | Client-side RGXF decoder (base64 → adjacency) |
| `events.svelte.ts` | `web/src/lib/stores/events.svelte.ts` | WebSocket store with auto-reconnect |

**Routes:** `/` (homepage), `/challenges` (list), `/challenges/[id]` (detail + viz + submit), `/records` (best-known), `/submit` (standalone)

## Server API

Port 3001, prefix `/api/`. SQLite at `./ramseynet.db`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check |
| `/api/challenges` | GET/POST | List or create challenges |
| `/api/challenges/{id}` | GET | Challenge detail + current record + record_graph RGXF |
| `/api/records` | GET | Best-known records |
| `/api/verify` | POST | Stateless graph verification |
| `/api/submit` | POST | Full lifecycle: verify + store + record update |
| `/api/events` | WS | OESP-1 event stream |

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier contract — JSON stdin/stdout, exit 0
- **OESP-1**: WebSocket event stream with monotonic sequence numbers

## Test Data

`test-vectors/small_graphs.json` contains C5, K5, E5, and Petersen graph with RGXF encodings.
`scripts/seed-ledger.sh` creates challenges and submits test graphs via the API.

## Phase Status

Phases 0–4 complete. Phase 5 (search workers + duels) is next.
