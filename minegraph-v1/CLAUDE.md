# MineGraph v1

Combinatorial graph search game with competitive leaderboards. Clean rewrite
of the RamseyNet prototype at `~/RamseyNet-dev/`.

## Quick Start

```bash
# CI
./run ci          # Full CI: fmt + clippy + tests
./run test        # Rust tests only

# Database setup (local Postgres on port 5432)
sudo -u postgres createuser minegraph
sudo -u postgres createdb -O minegraph minegraph
sudo -u postgres psql -c "ALTER USER minegraph WITH PASSWORD 'minegraph';"

# Environment (reads DATABASE_URL from env or .env)
cp .env.example .env

# Server (first run with --migrate, then just cargo run)
cargo run -p minegraph-server -- --migrate

# Worker
cargo run -p minegraph-worker -- --n 25 --beam-width 80 --max-depth 12

# CLI
cargo run -p minegraph-cli -- keygen --name "test"
cargo run -p minegraph-cli -- register-key
cargo run -p minegraph-cli -- submit --n 5 --graph6 'Dhc'
cargo run -p minegraph-cli -- leaderboard --n 25
cargo run -p minegraph-cli -- score --n 5 --graph6 'D~{'
cargo run -p minegraph-cli -- health
```

Other commands: `./run clippy`, `./run fmt`, `./run server`, `./run worker`.

Logging: `RUST_LOG=debug cargo run -p minegraph-server` (default: info).

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
- **Paley graph fallback** for cold-start seeding (no empty leaderboard problem)

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
    +-> minegraph-worker-core      (types, graph, scoring, identity, worker-api, strategies)
    +-> minegraph-strategies       (types, graph, scoring, worker-api)
    +-> minegraph-worker           (worker-api, worker-core, strategies, identity)
    +-> minegraph-cli              (types, graph, scoring, identity)
```

## Current Status

All 11 backend crates implemented and working end-to-end. 62 tests passing.

### Implemented
- `minegraph-types` — GraphCid (blake3), KeyId, Verdict
- `minegraph-graph` — AdjacencyMatrix, graph6 encode/decode, blake3 CID
- `minegraph-scoring` — NeighborSet, bitwise clique counting, CliqueHistogram, Goodman (cross-validated), GraphScore with Ord, violation delta, guilty edges, fast fingerprint
- `minegraph-identity` — Ed25519 keypair, signing, verification, key file I/O (single source of truth)
- `minegraph-store` — PostgreSQL models, 2 migrations, 20+ repository methods, lightweight leaderboard admission (no full-table rerank)
- `minegraph-server` — Axum API (13 endpoints): health, leaderboards, submit, verify, identity, SSE events, signed receipts, modular handlers
- `minegraph-worker-api` — SearchStrategy trait, SearchJob/Result, SearchObserver (CollectingObserver), WorkerCommand/Event/Status, ConfigParam
- `minegraph-strategies` — tree2 beam search ported from prototype (passes R(3,3)/n=5 and R(4,4)/n=17 tests), Paley graph init, perturb
- `minegraph-worker-core` — Engine loop with server client, leaderboard CID sync, biased seed sampling, Paley fallback for cold start, CollectingObserver for discovery capture
- `minegraph-worker` — Full CLI binary: n, target_k, target_ell, beam_width, max_depth, sample_bias, focused, offline, signing key, metadata
- `minegraph-cli` — init, keygen (with --output), whoami, register-key, score (local), submit, leaderboard, health

### TODO
1. Canonical labeling via nauty (CIDs currently non-canonical — isomorphic graphs get different CIDs)
2. Web apps (SvelteKit leaderboard + dashboard)
3. Evo strategy port
4. Production hardening (rate limiting, connection pool tuning)

## Key Design Decisions

| Decision | Choice |
|----------|--------|
| Graph format | graph6 (standard, well-known) |
| Hashing | blake3 |
| Database | PostgreSQL (sqlx, runtime queries) |
| Signatures | Required (Ed25519, no anonymous) |
| Scoring | Full k-clique histogram, lexicographic |
| Canonical labeling | nauty (C FFI) — not yet wired |
| Real-time updates | Server-Sent Events (SSE) |
| Web UI | Separate SvelteKit apps |
| Worker plugins | Trait-based, statically linked |
| Leaderboard admission | Lightweight: count-based rank, no full rerank |

## Scoring System

Golf-style (lower is better), lexicographic comparison:
1. For each k from max_k down to 3: `(max(red_k, blue_k), min(red_k, blue_k))`
2. Goodman gap (distance from theoretical minimum 3-clique count)
3. `1/|Aut(G)|` (more symmetric = lower = better)
4. CID bytes (deterministic tiebreaker)

Worker passes Ramsey target via strategy config: `target_k` and `target_ell`
(default: 5, 5 for R(5,5) search). Leaderboard is indexed by n only.

## Server API

Port 3001, prefix `/api/`. All endpoints return JSON.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check + server identity |
| `/api/submit` | POST | Full lifecycle: verify sig, score, store, admit, sign receipt |
| `/api/verify` | POST | Stateless scoring (no DB write, no sig required) |
| `/api/leaderboards` | GET | List all n values with counts |
| `/api/leaderboards/{n}` | GET | Paginated leaderboard (`?limit=&offset=`) |
| `/api/leaderboards/{n}/threshold` | GET | Admission score threshold |
| `/api/leaderboards/{n}/cids` | GET | Incremental CID sync (`?since=`) |
| `/api/leaderboards/{n}/graphs` | GET | Batch graph6 download |
| `/api/submissions/{cid}` | GET | Submission detail + receipt + score |
| `/api/keys` | POST | Register public key |
| `/api/keys/{key_id}` | GET | Identity info |
| `/api/keys/{key_id}/submissions` | GET | Submissions by identity |
| `/api/events` | GET | SSE stream (admission + submission events) |

## Server Configuration

All via env vars or CLI flags (clap `env =`):

| Var | Flag | Default | Description |
|-----|------|---------|-------------|
| `DATABASE_URL` | `--database-url` | `postgres://localhost/minegraph` | PostgreSQL URL |
| `PORT` | `--port` | `3001` | Listen port |
| `LEADERBOARD_CAPACITY` | `--leaderboard-capacity` | `500` | Max entries per n |
| `MAX_K` | `--max-k` | `5` | Max k for histogram scoring |
| `SERVER_KEY_PATH` | `--server-key` | (ephemeral) | Persistent server signing key |
| `RUST_LOG` | — | `info` | Log level |

## Worker Configuration

```
--n 25                    Target vertex count
--target-k 5              Clique size in graph (default 5)
--target-ell 5            Clique size in complement (default 5)
--beam-width 80           Beam candidates per depth
--max-depth 12            Search depth levels
--max-iters 100000        Iteration budget per round
--sample-bias 0.8         Leaderboard seed bias (0=uniform, 1=top)
--focused false           Focused edge flipping
--noise-flips 0           Random flips on seed
--offline                 Local-only (no server)
--signing-key PATH        Ed25519 key (or auto-detect .config/minegraph/key.json)
```

## Database

PostgreSQL with sqlx migrations in `migrations/`. Leaderboard PK is `(n, cid)`.
Rank is computed at insertion time, queries sort by `score_bytes` directly.

Tables: `identities`, `graphs`, `submissions`, `scores`, `leaderboard`,
`receipts`, `server_config`.

### Local setup

```bash
sudo -u postgres createuser minegraph
sudo -u postgres createdb -O minegraph minegraph
sudo -u postgres psql -c "ALTER USER minegraph WITH PASSWORD 'minegraph';"
```

### Persistent server key

```bash
cargo run -p minegraph-cli -- keygen --name "my-server" -o .config/minegraph/server-key.json
export SERVER_KEY_PATH=.config/minegraph/server-key.json
cargo run -p minegraph-server -- --migrate
```

## Prototype Reference

The RamseyNet prototype at `~/RamseyNet-dev/` has:
- Evolutionary SA strategy (evo) — not yet ported
- Fleet/experiment infrastructure — not yet ported
- GemView rendering — not yet ported
- SvelteKit leaderboard web app — to be rebuilt
- nauty canonical labeling + automorphism — to be ported

See `~/RamseyNet-dev/CLAUDE.md` for prototype details.

## Testing

62 tests across all crates. Run with `cargo test`.
Clippy clean (`-D warnings`), `cargo fmt` clean.
CI: `.github/workflows/ci.yml` (fmt + clippy + test).
