# RamseyNet

A permissionless protocol for distributed Ramsey graph search and deterministic generative graph art.

## Build Commands

```bash
# Rust
cargo build --all                    # Build all crates
cargo test --all                     # Run all tests
cargo clippy --all-targets -- -D warnings  # Lint
cargo run -p ramseynet-server        # Start API server (port 3001)

# WASM verifier
cargo build --target wasm32-wasip1 -p ramseynet-verifier --release --bin ovwc1

# Web app
cd web && pnpm install               # Install dependencies
cd web && pnpm dev                   # Dev server (port 5173)
cd web && pnpm build                 # Production build
```

## Architecture

Monorepo: Rust workspace (`crates/`) + SvelteKit (`web/`).

**Crate dependency order:** `types` -> `graph` -> `verifier` -> `ledger` -> `server` <- `search`

| Crate | Purpose |
|-------|---------|
| `ramseynet-types` | Shared newtypes: GraphCid, ChallengeId, RamseyParams, Verdict |
| `ramseynet-graph` | RGXF encode/decode, AdjacencyMatrix (packed upper-tri bitstring), SHA-256 CID |
| `ramseynet-verifier` | Clique/independent-set detection, OVWC-1 WASM binary, canonical witnesses |
| `ramseynet-ledger` | SQLite schema, transaction types, canonical state derivation |
| `ramseynet-server` | Axum REST + WebSocket, artifact store, verifier host |
| `ramseynet-search` | Greedy, local search, simulated annealing, worker loop |

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier takes JSON stdin, writes JSON stdout, exit 0
- **OESP-1**: WebSocket event stream with monotonic sequence numbers
- **ORS-1.0**: Deterministic render from CID seed, quantized circle layout

## Server

- Default port: 3001
- DB: SQLite at `./ramseynet.db`
- API prefix: `/api/`
- WebSocket events: `/api/events`

## Test Vectors

- `test-vectors/small_graphs.json` - Known graphs with precomputed CIDs
- `test-vectors/verify_requests.json` - OVWC-1 request/response pairs
