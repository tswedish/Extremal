# MineGraph v1

Combinatorial graph search game with competitive leaderboards and deterministic
generative graph art. Players run search workers that discover high-scoring
graphs and submit them to a central leaderboard server, competing for rank.

## Prerequisites

- **Rust** (stable, via [rustup](https://rustup.rs/))
- **PostgreSQL** 14+ (local or remote)

## Setup

### 1. Database

If you have a local PostgreSQL instance running (e.g., system Postgres on port 5432):

```bash
sudo -u postgres createuser minegraph
sudo -u postgres createdb -O minegraph minegraph
sudo -u postgres psql -c "ALTER USER minegraph WITH PASSWORD 'minegraph';"
```

Or with Docker (uses port 5433 to avoid conflict with local Postgres):

```bash
docker-compose up -d
# DATABASE_URL becomes: postgres://minegraph:minegraph@localhost:5433/minegraph
```

### 2. Environment

Copy the example env file and edit if needed:

```bash
cp .env.example .env
```

The defaults work with the local database created above:

```
DATABASE_URL=postgres://minegraph:minegraph@localhost/minegraph
PORT=3001
```

### 3. Start the server

First run (creates tables):

```bash
cargo run -p minegraph-server -- --migrate
```

Subsequent runs (database already exists):

```bash
cargo run -p minegraph-server
```

The server reads `DATABASE_URL` and `PORT` from the environment (or `.env`).
You can also pass them explicitly:

```bash
cargo run -p minegraph-server -- \
  --database-url 'postgres://minegraph:minegraph@localhost/minegraph'
```

The `--migrate` flag is idempotent — safe to always include, but not required
after the first run.

Control log verbosity with `RUST_LOG` (default is `info`):

```bash
RUST_LOG=debug cargo run -p minegraph-server                          # verbose
RUST_LOG=warn cargo run -p minegraph-server                           # quiet
RUST_LOG=info,minegraph_server=debug cargo run -p minegraph-server    # server detail only
```

By default, the server generates an **ephemeral** signing identity on each
startup (a new key_id every time). For production or persistent testing, create
a dedicated server key:

```bash
# Generate a server key (saved to a specific file)
cargo run -p minegraph-cli -- keygen --name "my-server" \
  --output .config/minegraph/server-key.json

# Start the server with the persistent key
cargo run -p minegraph-server -- --server-key .config/minegraph/server-key.json
```

The server uses this key to sign verification receipts. A persistent key means
receipts remain verifiable across server restarts.

You can also set this via environment:

```bash
export SERVER_KEY_PATH=.config/minegraph/server-key.json
cargo run -p minegraph-server
```

### 4. Generate a worker identity

```bash
cargo run -p minegraph-cli -- keygen --name "my-worker"
cargo run -p minegraph-cli -- register-key --server http://localhost:3001
```

### 5. Score and submit graphs

```bash
# Score a graph locally (no server needed)
cargo run -p minegraph-cli -- score --n 5 --graph6 'Dhc'

# Submit to the server
cargo run -p minegraph-cli -- submit --n 5 --graph6 'Dhc' \
  --server http://localhost:3001

# Check the leaderboard
cargo run -p minegraph-cli -- leaderboard --n 5 --server http://localhost:3001

# Server health
cargo run -p minegraph-cli -- health --server http://localhost:3001
```

## Development

```bash
./run ci          # Full CI: fmt + clippy + tests
./run test        # Rust tests only
./run clippy      # Lint
./run fmt         # Format code
./run server      # Start API server (needs DATABASE_URL env or --database-url)
```

## Architecture

Rust workspace with 11 crates. The server is a pure REST API; web UIs are
separate applications.

```
minegraph-types          Core newtypes: GraphCid, KeyId, Verdict
minegraph-graph          AdjacencyMatrix, graph6 encode/decode, blake3 CID
minegraph-scoring        Clique histogram, Goodman gap, GraphScore
minegraph-identity       Ed25519 signing (shared by server, worker, CLI)
minegraph-store          PostgreSQL data layer (sqlx)
minegraph-server         Axum REST API + SSE events
minegraph-worker-api     SearchStrategy trait, job/result types
minegraph-worker-core    Worker engine (TODO)
minegraph-strategies     Search strategies: tree2, evo (TODO)
minegraph-worker         Worker CLI binary (TODO)
minegraph-cli            CLI tool: keygen, register, submit, score, query
```

### Crate dependency order

`types` -> `graph` -> `scoring` -> `identity` -> `store` -> `server`

`types` -> `graph` -> `worker-api` -> `{strategies, worker-core}` -> `worker`

### Key design choices

| Area | Choice |
|------|--------|
| Graph format | graph6 (standard) |
| Hashing | blake3 |
| Database | PostgreSQL via sqlx |
| Signatures | Required (Ed25519, no anonymous) |
| Scoring | Full k-clique histogram, lexicographic |
| Real-time | Server-Sent Events (SSE) |
| Web UI | Separate SvelteKit apps (not built yet) |

## API

Port 3001, prefix `/api/`.

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check + server identity |
| `/api/submit` | POST | Submit graph (requires signature) |
| `/api/verify` | POST | Stateless scoring (no DB write) |
| `/api/leaderboards` | GET | List all leaderboards by n |
| `/api/leaderboards/{n}` | GET | Paginated leaderboard |
| `/api/leaderboards/{n}/threshold` | GET | Admission score threshold |
| `/api/leaderboards/{n}/cids` | GET | Incremental CID sync |
| `/api/leaderboards/{n}/graphs` | GET | Batch graph6 download |
| `/api/submissions/{cid}` | GET | Submission detail + receipt |
| `/api/keys` | POST | Register public key |
| `/api/keys/{key_id}` | GET | Identity info |
| `/api/keys/{key_id}/submissions` | GET | Submissions by identity |
| `/api/events` | GET | SSE stream of leaderboard events |

## Scoring System

Golf-style ranking (lower is better). For a graph G on n vertices:

1. **k-clique histogram**: for each k from max_k down to 3,
   compare `(max(red_k, blue_k), min(red_k, blue_k))` where red = cliques in G,
   blue = cliques in complement(G)
2. **Goodman gap**: actual monochromatic triangles minus theoretical minimum
3. **Automorphism**: `1/|Aut(G)|` (more symmetric = better)
4. **CID**: blake3 hash tiebreaker (lower wins)

## Status

The server, CLI, store, and scoring system are fully implemented. The search
worker (strategies + engine loop) is the next major piece to port from the
RamseyNet prototype.

## License

MIT
