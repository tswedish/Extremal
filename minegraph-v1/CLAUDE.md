# MineGraph v1

Combinatorial graph search game with competitive leaderboards. Clean rewrite
of the RamseyNet prototype at `~/RamseyNet-dev/`.

## Quick Start

```
./run ci          # Full CI: fmt + clippy + tests
./run test        # Rust tests only
./run server      # API server (requires Postgres)
./run worker      # Search worker
./run cli         # CLI tool (keygen, register, query)
./run fmt         # Format code
```

## Architecture

Rust workspace (`crates/`) with 11 crates. Key differences from prototype:
- **graph6** format (not RGXF)
- **blake3** hashing (not SHA-256)
- **PostgreSQL** via sqlx (not SQLite)
- **Full k-clique histogram** scoring (not 4-tier)
- **Signatures required** (no anonymous submissions)
- **Shared identity crate** (no signing code duplication)
- **Server is API-only** (web apps are separate)
- **Leaderboards indexed by n only** (not k,ell,n)
- **SSE for real-time updates** (not WebSocket)

## Crate Dependency Graph

```
minegraph-types                    (leaf — no internal deps)
    |
    +-> minegraph-graph            (types)
    |       |
    |       +-> minegraph-scoring  (types, graph)
    |       |
    |       +-> minegraph-identity (types)
    |
    +-> minegraph-store            (types, graph, scoring, identity)
    +-> minegraph-server           (types, graph, scoring, identity, store)
    +-> minegraph-worker-api       (types, graph)
    +-> minegraph-worker-core      (types, graph, scoring, identity, worker-api)
    +-> minegraph-strategies       (types, graph, scoring, worker-api)
    +-> minegraph-worker           (worker-api, worker-core, strategies, identity)
    +-> minegraph-cli              (types, graph, identity)
```

## Current Status

Phase 1 (foundation crates) complete. Phase 2-5 in progress.

### Completed
- `minegraph-types` — GraphCid, KeyId, Verdict
- `minegraph-graph` — AdjacencyMatrix, graph6 encode/decode, blake3 CID
- `minegraph-scoring` — NeighborSet, clique counting, histogram, Goodman gap, GraphScore with Ord
- `minegraph-identity` — Ed25519 keypair generation, signing, verification (single source of truth)
- `minegraph-store` — PostgreSQL models, migrations, Store with PgPool
- `minegraph-worker-api` — SearchStrategy trait, SearchJob, SearchResult, SearchObserver
- All remaining crates stubbed and compiling

### Next to Implement
1. `minegraph-server` — Axum API handlers, SSE, signed receipts
2. `minegraph-strategies` — Port tree2 from prototype
3. `minegraph-worker-core` — Engine loop, server client
4. `minegraph-worker` — CLI binary
5. `minegraph-cli` — keygen, register, query
6. Web apps (SvelteKit, separate from server)
7. Docker, CI, cloud deployment

## Key Design Decisions

| Decision | Choice |
|----------|--------|
| Graph format | graph6 (standard, well-known) |
| Hashing | blake3 |
| Database | PostgreSQL (sqlx, compile-time checked) |
| Signatures | Required (Ed25519, no anonymous) |
| Scoring | Full k-clique histogram, lexicographic |
| Canonical labeling | nauty (C FFI, same as prototype) |
| Real-time updates | Server-Sent Events (SSE) |
| Web UI | Separate SvelteKit apps (leaderboard + dashboard) |
| Worker plugins | Trait-based, statically linked |

## Scoring System

Golf-style (lower is better), lexicographic comparison:
1. For each k from max_k down to 3: `(max(red_k, blue_k), min(red_k, blue_k))`
2. Goodman gap (distance from theoretical minimum 3-clique count)
3. `1/|Aut(G)|` (more symmetric = lower = better)
4. CID bytes (deterministic tiebreaker)

## Prototype Reference

The RamseyNet prototype at `~/RamseyNet-dev/` has proven implementations of:
- Bitwise incremental beam search (tree2)
- Evolutionary simulated annealing (evo)
- Fleet/experiment infrastructure
- GemView rendering
- SvelteKit leaderboard web app
- Ed25519 signing (duplicated in 3 places — fixed in v1)

See `~/RamseyNet-dev/CLAUDE.md` for prototype details.

## Database

PostgreSQL with sqlx migrations in `migrations/`. Tables:
- `identities` — registered Ed25519 public keys
- `graphs` — deduplicated graphs (CID + graph6)
- `submissions` — signed submissions
- `scores` — precomputed histogram scores
- `leaderboard` — ranked entries per n
- `receipts` — server-signed verification results
- `server_config` — server-level configuration

## Testing

53 tests across the foundation crates. Run with `cargo test`.
Clippy clean, `cargo fmt` clean.
