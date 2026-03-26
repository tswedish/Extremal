# Extremal v1

Combinatorial graph search game with competitive leaderboards.

## Quick Start

```bash
# CI
./run ci          # Full CI: fmt + clippy + tests
./run test        # Rust tests only

# Database setup (local Postgres on port 5432)
sudo -u postgres createuser extremal
sudo -u postgres createdb -O extremal extremal
sudo -u postgres psql -c "ALTER USER extremal WITH PASSWORD 'extremal';"

# Environment (reads DATABASE_URL from env or .env)
cp .env.example .env

# Server (first run with --migrate, then just cargo run)
cargo run -p extremal-server -- --migrate

# Dashboard relay server (for worker monitoring)
./run dashboard

# Dashboard web UI (SvelteKit, port 5174)
./run dashboard-ui

# Server web app (SvelteKit, port 5173)
./run web-dev

# Worker (with dashboard connection)
cargo run -p extremal-worker -- --n 25 --beam-width 80 --max-depth 12 --dashboard ws://localhost:4000/ws/worker

# Experiment fleet (8 diverse workers, release build)
./scripts/experiment.sh 25

# Fleet (uniform workers)
./scripts/fleet.sh --workers 4 --n 25 --release --dashboard ws://localhost:4000/ws/worker

# CLI
cargo run -p extremal-cli -- keygen --name "test"
cargo run -p extremal-cli -- register-key
cargo run -p extremal-cli -- submit --n 5 --graph6 'Dhc'
cargo run -p extremal-cli -- leaderboard --n 25
cargo run -p extremal-cli -- score --n 5 --graph6 'D~{'
cargo run -p extremal-cli -- health

# Worker management (via dashboard relay)
cargo run -p extremal-cli -- workers --relay http://localhost:4000 list
cargo run -p extremal-cli -- workers status fleet-1
cargo run -p extremal-cli -- workers config fleet-1
cargo run -p extremal-cli -- workers set fleet-1 beam_width=200 sample_bias=0.5
cargo run -p extremal-cli -- workers pause fleet-1
cargo run -p extremal-cli -- workers resume fleet-1
cargo run -p extremal-cli -- workers stop fleet-1
```

Other commands: `./run clippy`, `./run fmt`, `./run server`, `./run worker`, `./run dashboard`, `./run dashboard-ui`, `./run web-dev`, `./run web-build`.

```bash
# Docker (local integration testing)
docker compose up --build        # Postgres + server container
docker compose down              # Tear down
```

Logging: `RUST_LOG=debug cargo run -p extremal-server` (default: info).

## Architecture

Rust workspace (`crates/`) with 12 crates + 2 SvelteKit web apps + shared component package:

- **graph6** format for graph encoding
- **blake3** hashing for CIDs
- **PostgreSQL** via sqlx
- **Full k-clique histogram** scoring (lexicographic)
- **Ed25519 signatures required** (no anonymous submissions)
- **Server is API-only** (web apps are separate)
- **Leaderboards indexed by n only**
- **SSE for real-time updates** (enriched with graph6/scores for visualization)
- **Paley graph fallback** for cold-start seeding
- **Dashboard relay server** for worker monitoring (separate from leaderboard server)
- **WebSocket telemetry** from workers to dashboard relay
- **Ed25519 challenge/response auth** for dashboard worker connections
- **Worker HTTP API** for runtime parameter adjustment and pause/resume/stop
- **CLI worker management** via dashboard relay discovery + direct worker API
- **Production hardening**: rate limiting (per-IP, tiered), request timeouts, CORS config, graceful shutdown, DB pool tuning
- **Containerized** via Docker (multi-stage build for server)
- **Cloud Run ready**: advisory locks for dedup, configurable pool size, SIGTERM handling

## Crate Dependency Graph

```
extremal-types                    (leaf — no internal deps)
    |
    +-> extremal-graph            (types)
    |       |
    |       +-> extremal-scoring  (types, graph)
    |       |
    |       +-> extremal-identity (types)
    |
    +-> extremal-store            (types, graph, scoring, identity)
    +-> extremal-server           (types, graph, scoring, identity, store)
    +-> extremal-worker-api       (types, graph)
    +-> extremal-worker-core      (types, graph, scoring, identity, worker-api, strategies, dashboard)
    +-> extremal-strategies       (types, graph, scoring, worker-api)
    +-> extremal-worker           (worker-api, worker-core, strategies, identity)
    +-> extremal-cli              (types, graph, scoring, identity)
    +-> extremal-dashboard        (identity) — standalone dashboard relay server
```

## Web Apps & Frontend

```
web/                     Server web app (SvelteKit, port 5173)
dashboard/               Worker dashboard app (SvelteKit, port 5174)
packages/shared/         Shared components (@extremal/shared)
  src/components/
    GemView.svelte         Diamond-rotated adjacency matrix visualization
    GemViewSquare.svelte   Rain column variant (opacity, glow, click props)
    GemPopup.svelte        Click-to-expand detail modal
  src/graph6.ts            graph6 decoder
  src/types.ts             Shared TypeScript types
```

npm workspaces: root `package.json` manages `packages/shared`, `web`, `dashboard`.

## Current Status

All 12 backend crates implemented and working end-to-end. 86 tests passing (including property-based tests).

### Implemented
- `extremal-types` — GraphCid (blake3), KeyId, Verdict
- `extremal-graph` — AdjacencyMatrix, graph6 encode/decode, blake3 CID
- `extremal-scoring` — NeighborSet, bitwise clique counting, CliqueHistogram, Goodman (cross-validated), GraphScore with Ord, violation delta, guilty edges, fast fingerprint, canonical labeling via nauty
- `extremal-identity` — Ed25519 keypair, signing, verification, key file I/O (single source of truth)
- `extremal-store` — PostgreSQL models, 3 migrations, 30+ repository methods, lightweight leaderboard admission (no full-table rerank), advisory locks for distributed coordination, health check with pool stats
- `extremal-server` — Axum API: health, leaderboards, submit, verify, identity, SSE events, signed receipts. Production hardened: rate limiting on writes (300/s), 30s timeouts, configurable CORS, graceful shutdown, input validation, DB pool tuning, advisory-locked snapshot dedup.
- `extremal-worker-api` — SearchStrategy trait, SearchJob/Result, SearchObserver (CollectingObserver), WorkerCommand/Event/Status, ConfigParam (with `adjustable` flag)
- `extremal-strategies` — tree2 beam search (passes R(3,3)/n=5 and R(4,4)/n=17 tests), Paley graph init, perturb. ConfigParam adjustability: beam_width/max_depth/focused=adjustable, target_k/target_ell=init-only
- `extremal-worker-core` — Engine loop with server client, leaderboard CID sync, biased seed sampling, Paley fallback for cold start, DashboardObserver for real-time telemetry, priority-sorted submit buffer (best graphs submitted first), throttled progress events (4 Hz), **command channel** (pause/resume/stop/config-update between rounds), **HTTP API server** (status, config, control), **EngineSnapshot** watch channel for API
- `extremal-worker` — Full CLI binary: n, target_k, target_ell, beam_width, max_depth, sample_bias, focused, offline, signing key, metadata, dashboard URL, **--api-port** for control API
- `extremal-cli` — init, keygen (with --output), whoami, register-key, score (local), submit, leaderboard, health, **workers** (list/status/config/set/pause/resume/stop via relay discovery + direct worker API)
- `extremal-dashboard` — Standalone Axum relay server: worker WebSocket endpoint, browser WebSocket endpoint (multiplexed), REST API for worker listing, **Ed25519 challenge/response auth** (default open, verified flag), static file serving, **api_addr** in worker info for CLI discovery
- **Server web app** (`web/`) — SvelteKit: home, leaderboards (paginated with GemView), activity dashboard (submission-inferred), rain visualization (SSE-driven), submission detail, identity profiles
- **Worker dashboard** (`dashboard/`) — SvelteKit: monitor mode (live worker stats, progress bars, gem thumbnails), rain mode (vertical gem columns per worker, current search at top, best-found pool below), controls (gem size, fade duration 10m-8h, history depth 10-200), fullscreen mode
- **Shared components** (`packages/shared/`) — GemView (diamond adjacency matrix), GemViewSquare (rain variant), GemPopup (detail modal), graph6 decoder

### Deployed
- **Server**: Cloud Run (`api.extremal.online`) + Cloud SQL (Postgres 18)
- **Web app**: Cloud Run (`extremal.online`) — static SvelteKit + nginx API proxy
- **Dashboard/workers**: local only (not deployed)
- **Auto-deploy**: `cloudbuild.yaml` ready, needs Cloud Build trigger on `main`

### TODO
1. Connect Cloud Build trigger to GitHub repo (auto-deploy on merge to main)
2. New search strategy — explore alternatives competitive with tree2
3. Server integration tests (against test database)

## Key Design Decisions

| Decision | Choice |
|----------|--------|
| Graph format | graph6 (standard, well-known) |
| Hashing | blake3 |
| Database | PostgreSQL (sqlx, runtime queries) |
| Signatures | Required (Ed25519, no anonymous) |
| Scoring | Full k-clique histogram, lexicographic, golf-style |
| Canonical labeling | nauty (C FFI) — wired into scoring and worker |
| Real-time updates | SSE (server), WebSocket (dashboard relay) |
| Web UI | Separate SvelteKit apps (server + dashboard) |
| Worker monitoring | Dedicated dashboard relay server (not on leaderboard server) |
| Worker plugins | Trait-based, statically linked |
| Leaderboard admission | Lightweight: count-based rank, no full rerank |
| Submit buffer | Priority-sorted (best score first) |

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
| `/api/health` | GET | Health check + server identity + pool stats |
| `/api/submit` | POST | Full lifecycle: verify sig, score, store, admit, sign receipt |
| `/api/verify` | POST | Stateless scoring (no DB write, no sig required) |
| `/api/leaderboards` | GET | List all n values with counts |
| `/api/leaderboards/{n}` | GET | Paginated leaderboard (`?limit=&offset=`) |
| `/api/leaderboards/{n}/threshold` | GET | Admission score threshold |
| `/api/leaderboards/{n}/cids` | GET | Incremental CID sync (`?since=`) |
| `/api/leaderboards/{n}/graphs` | GET | Batch graph6 download |
| `/api/submissions/{cid}` | GET | Submission detail + receipt + score |
| `/api/keys` | POST | Register public key |
| `/api/keys/{key_id}` | GET | Identity info (with leaderboard_limit param) |
| `/api/keys/{key_id}/submissions` | GET | Submissions by identity |
| `/api/events` | GET | SSE stream (enriched admission + submission events) |

## Dashboard Relay Server

Port 4000. Separate from the leaderboard server.

| Endpoint | Type | Description |
|----------|------|-------------|
| `/ws/worker` | WebSocket | Workers register and stream telemetry |
| `/ws/ui` | WebSocket | Browser receives multiplexed worker events |
| `/api/workers` | GET | List connected workers |
| `/api/config` | GET | Dashboard configuration |

**Auth**: Ed25519 challenge/response. Server sends 32-byte random nonce on connect. Worker signs nonce with Ed25519 key, includes `public_key_hex` + `nonce_signature` in Register message. Server verifies key_id matches public key and signature is valid. Default mode (no allow-list): accepts all, logs verification result (`verified: true/false`). Allow-list mode: rejects unverified or unlisted keys.

**Protocol**: Workers send `Register` (with optional auth fields + `api_addr`), `Progress`, `Discovery`, `RoundComplete` messages. Dashboard relays to browser as `WorkerConnected` (with `verified`, `api_addr`), `WorkerDisconnected`, `WorkerEvent` envelopes.

## Worker HTTP API

Each worker runs a local Axum HTTP API server for runtime control. Port is configurable via `--api-port` (default: 0 = auto-assign). The worker advertises its API address via the dashboard relay's `api_addr` field.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/status` | GET | Engine state, round, metrics |
| `/api/config` | GET | All params with values + adjustability |
| `/api/config` | POST | Patch adjustable params (JSON body) |
| `/api/pause` | POST | Pause after current round |
| `/api/resume` | POST | Resume from paused state |
| `/api/stop` | POST | Graceful shutdown |

**Adjustable params** (can be changed at runtime between rounds):
- Engine: `max_iters`, `sample_bias`, `noise_flips`, `max_submissions_per_round`
- tree2 strategy: `beam_width`, `max_depth`, `focused`

**Init-only params** (fixed at startup): `n`, `target_k`, `target_ell`, `server_url`, `strategy_id`

Commands are processed between rounds (not mid-search). A round typically takes 0.5–10s.

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
--signing-key PATH        Ed25519 key (or auto-detect .config/extremal/key.json)
--dashboard URL            Dashboard relay WebSocket URL
--api-port PORT            Worker control API port (0=auto, default 0)
--metadata JSON            Metadata JSON (max 4KB, attached to submissions)
```

### Tuning guide (from experiments on n=25)

Best performers use **moderate noise (1-3 flips)** and **low sample bias (0.3-0.6)**:
- Wide beam (150-200) + shallow depth (8-10): most discoveries, broad exploration
- Focused mode + noise (2 flips): high admission rate, surgical improvements
- Deep beam (40-60) + high depth (16-20): fewer but higher-quality discoveries
- High noise (>5 flips): too destructive, poor results

## Database

PostgreSQL with sqlx migrations in `migrations/`. Leaderboard PK is `(n, cid)`.
Rank is computed at insertion time, queries sort by `score_bytes` directly.

Tables: `identities`, `graphs`, `submissions`, `scores`, `leaderboard`,
`receipts`, `server_config`.

### Local setup

```bash
sudo -u postgres createuser extremal
sudo -u postgres createdb -O extremal extremal
sudo -u postgres psql -c "ALTER USER extremal WITH PASSWORD 'extremal';"
```

### Persistent server key

```bash
cargo run -p extremal-cli -- keygen --name "my-server" -o .config/extremal/server-key.json
export SERVER_KEY_PATH=.config/extremal/server-key.json
cargo run -p extremal-server -- --migrate
```

## Server Configuration

| Env Var / Flag | Default | Description |
|---------------|---------|-------------|
| `PORT` / `--port` | 3001 | HTTP listen port |
| `DATABASE_URL` / `--database-url` | `postgres://localhost/extremal` | PostgreSQL connection |
| `LEADERBOARD_CAPACITY` / `--leaderboard-capacity` | 500 | Max entries per leaderboard |
| `MAX_K` / `--max-k` | 5 | Max clique size for scoring |
| `MAX_N` / `--max-n` | 62 | Max graph vertex count (graph6 limit) |
| `DB_MAX_CONNECTIONS` / `--db-max-connections` | 10 | Database pool size (use 5 on Cloud Run) |
| `SERVER_KEY_PATH` / `--server-key` | (ephemeral) | Ed25519 signing key path |
| `ALLOWED_ORIGINS` / `--allowed-origins` | (permissive) | CORS origins (comma-separated) |
| `--migrate` | false | Run DB migrations on startup |

**Rate limiting**: 300 req/s per IP on write endpoints (submit/verify/register-key). Read-only endpoints are not rate-limited. Request timeout: 30s.

## Deployment

**Live URLs:**
- Server API: `https://api.extremal.online`
- Web app: `https://extremal.online`
- Domain: `extremal.online` (Wix DNS → Cloud Run)

```bash
# Local Docker
docker compose up --build

# Cloud Run (manual deploy — auto-deploy via cloudbuild.yaml pending)
gcloud builds submit --config=/dev/stdin --timeout=600s <<'EOF'
steps:
  - name: 'gcr.io/cloud-builders/docker'
    args: ['build', '-f', 'Dockerfile.server', '-t', 'us-central1-docker.pkg.dev/extremal/extremal/server:TAG', '.']
images: ['us-central1-docker.pkg.dev/extremal/extremal/server:TAG']
EOF
gcloud run services update extremal-server --region us-central1 --image=...

# Workers submit to production
cargo run --release -p extremal-worker -- --server https://api.extremal.online --n 25
```

**GCP resources:** Cloud SQL (`extremal-database-0`, Postgres 18), Artifact Registry (`extremal`), Secret Manager (`extremal-server-key`, `extremal-db-password`).

**Scaling notes**: SSE is instance-local (clients reconnect). Snapshots use advisory locks. CID polling is cross-instance safe.

## Testing

86 tests across all crates. Run with `cargo test`.
Includes property-based tests for graph6 encode/decode roundtrip.
Clippy clean (`-D warnings`), `cargo fmt` clean.
CI: `.github/workflows/ci.yml` (fmt + clippy + test).

## Performance Notes

Worker→dashboard telemetry is throttled at multiple levels:
- Worker: Progress events at 4 Hz max, Discovery events capped at 20/round
- Worker→relay channel: bounded (64 msgs), `try_send` drops excess
- Relay broadcast: 256 capacity, lagged receivers skip
- Browser: messages batched per 500ms flush, Progress deduped per worker
- Submit buffer: priority-sorted by score (best first)
