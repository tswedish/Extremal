# MineGraph (formerly RamseyNet)

Distributed Ramsey graph search, competitive leaderboards, and deterministic
generative graph art ("MineGraph Gems").

## Quick Start

```
./run ci          # Full CI: clippy + tests + web build
./run test        # Rust tests only
./run server      # API server on :3001
./run server-log  # API server with file logging
./run web-dev     # SvelteKit dev server on :5173
./run search      # Search worker (default: tree2, idle mode)
./run fleet       # Launch 16-worker fleet (production search)
./run fleet --sweep  # Fleet with hyperparameter sweep
./run experiment  # Head-to-head strategy comparison
./run seed        # Seed DB with test data
```

Other commands: `clippy`, `build`, `web` (production build), `bench` (criterion benchmarks).

Add `--release` to `server`, `search`, `fleet`, `build`, `test` for optimized builds.

### Production Search (the main thing)

```bash
# Terminal 1: server
./run server --release --leaderboard-capacity 2000

# Terminal 2: fleet of 16 workers (best known config)
./run fleet --workers 16 --base-port 9000 \
  --beam-width 80 --max-depth 12 --sample-bias 0.8

# Or sweep across hyperparameter profiles
./run fleet --sweep --base-port 9000
```

### Search Worker

```
./run search --k 5 --ell 5 --n 25                       # tree2 (default), default server
./run search --k 5 --ell 5 --n 25 --strategy tree       # original beam search
./run search --k 5 --ell 5 --n 25 --strategy evo        # evolutionary SA
./run search --k 3 --ell 4 --n 8 --server http://remote:3001 --max-iters 50000
./run search --k 4 --ell 4 --n 17 --offline --port 8080
```

Options: `--strategy {tree|tree2|evo|all}`, `--init {perturbed-paley|paley|random|leaderboard}`, `--noise-flips N`, `--max-iters N`, `--beam-width N`, `--max-depth N`, `--port PORT`, `--offline`, `--no-backoff`, `--sample-bias F`, `--leaderboard-sample-size N`, `--collector-capacity N`, `--max-known-cids N`.

## Experiment Loop

The standard development cycle for improving search strategies:

1. **Identify** the next algorithmic change (see `docs/LITERATURE_AND_IDEAS.md`)
2. **Implement** the change as a new strategy or tree2 variant
3. **Run** `./run fleet --sweep` or `./run experiment` against production server
4. **Analyze** with `./scripts/analyze_experiment.sh logs/fleet-*/`
5. **Log** results in `experiments/ENNN.md`
6. **Decide** — promote the winner, identify next change, repeat

### Fleet Commands

```bash
# Production fleet (16 workers, best config)
./run fleet --workers 16 --base-port 9000 \
  --beam-width 80 --max-depth 12 --sample-bias 0.8

# Hyperparameter sweep (6 profiles, auto-distributed)
./run fleet --sweep --base-port 9000

# Check progress without stopping
cat logs/fleet-*/status.txt

# Full analysis after stopping
./scripts/analyze_experiment.sh logs/fleet-*/
```

### Key Metrics

- **Admits/hr** — leaderboard admissions per hour (primary measure of strategy quality)
- **Admit/Wk** — admissions per worker (for comparing profiles in a sweep)
- **Admission rate** — % of submissions that get admitted (indicates leaderboard headroom)
- **Disc/Rnd** — discoveries per round (indicates search breadth)

## Current Strategy Status

| Strategy | ID | Status | Description |
|----------|-----|--------|-------------|
| **tree2** | `tree2` | **Production default** | Incremental beam search with bitwise neighbor bitmasks, flip-score-unflip, 64-bit fingerprint dedup |
| tree | `tree` | Reference/ablation | Original beam search with full clique recount per candidate. 11x slower than tree2. |
| evo | `evo` | Experimental | Evolutionary SA with population, cross-round persistence, leaderboard immigrant injection |

**Best known hyperparameters** (from E004, 7-hour sweep):
- beam_width=80, max_depth=12, sample_bias=0.8 ("focused" profile)
- Top-heavy leaderboard sampling (bias=0.8) is the single most important knob

## Architecture

Rust workspace (`crates/`) + SvelteKit 2 (`web/`).

**Crate dependency order:** `types` → `graph` → `verifier` → `worker-api` → `{strategies, worker-core}` → `worker` ; `{types, graph}` → `ledger` ; `{verifier, ledger}` → `server`

| Crate | Purpose |
|-------|---------|
| `ramseynet-types` | Shared newtypes: GraphCid, RamseyParams, Verdict |
| `ramseynet-graph` | RGXF encode/decode, AdjacencyMatrix, neighbor bitmasks, SHA-256 CID |
| `ramseynet-verifier` | Clique/independent-set detection, 4-tier scoring, automorphism |
| `ramseynet-ledger` | SQLite ledger: submissions, receipts, leaderboards |
| `ramseynet-server` | Axum REST API, full submit lifecycle |
| `ramseynet-worker-api` | Search strategy trait, job/result schemas, observer interface |
| `ramseynet-worker-core` | Worker engine: leaderboard sync, submission pipeline, init |
| `ramseynet-strategies` | Search strategies: tree, tree2, evo; shared incremental module |
| `ramseynet-worker` | CLI binary + worker web-app (visualization dashboard) |

## Leaderboard System

Every valid (K,L,n) triple implicitly defines a leaderboard of capacity 500 (configurable via `--leaderboard-capacity` on the server). No explicit "challenges" — submit directly with `{k, ell, n, graph}`. Capacity can be changed at server restart — shrinking trims the lowest-ranked entries automatically.

**Scoring** (4-tier lexicographic, lower is better):
- **T1**: `(max(C_omega, C_alpha), min(C_omega, C_alpha))` — clique counts, lowest wins
- **T2**: Goodman gap — distance from Goodman's minimum monochromatic triangles, lowest wins (0 = optimal)
- **T3**: `|Aut(G)|` — automorphism group order, highest wins
- **T4**: CID — deterministic tiebreaker, smallest wins

K ≤ L canonical form enforced everywhere (R(K,L) = R(L,K)).

## Web App (SvelteKit 2 / Svelte 5)

**Components** in `web/src/lib/components/`:

| Component | Purpose |
|-----------|---------|
| `GemView` | Diamond adjacency matrix with hash-derived palette (MineGraph Gem) |
| `MatrixView` | Canvas adjacency matrix with witness overlay |
| `CircleLayout` | SVG circle graph (ORS-1.0) |
| `GraphThumb` | Small canvas thumbnail of adjacency matrix |
| `SubmitForm` | K/L/N inputs + RGXF paste + preview + submit |

**Routes:**

| Route | Purpose |
|-------|---------|
| `/` | Homepage with #1 gem showcase, health badge, nav cards |
| `/leaderboards` | Browse by (K,L) pairs, drill into n values |
| `/leaderboards/[k]/[l]/[n]` | Ranked table with gem + matrix + circle viz for top graph |
| `/submissions/[cid]` | Submission detail: verdict, witness, gem + graph viz, rank |
| `/submit` | Standalone graph submission form |

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

## Key Documents

| File | Purpose |
|------|---------|
| `NEXT_STEPS.md` | Current priorities and what to build next |
| `experiments/E001-E004.md` | Experiment logs with results and analysis |
| `docs/LITERATURE_AND_IDEAS.md` | Paper summaries + prioritized strategy ideas |
| `docs/STRATEGY_DEV_PLAN.md` | Strategy history and tree2 improvement roadmap |
| `docs/SIGNING_DESIGN.md` | Ed25519 identity system design (not yet built) |

## Key Specs

- **RGXF**: Packed upper-triangular adjacency bitstring, SHA-256 content addressed
- **OVWC-1**: Verifier contract — JSON stdin/stdout, exit 0
- **Gem rendering**: `minegraph_gem_v3.py` (Python) and `GemView.svelte` (web component)

## Test Data

- `test-vectors/small_graphs.json` — C5, K5, E5, Petersen, Wagner with RGXF encodings
- `scripts/seed-ledger.sh` — submits test graphs via the API

## Identity & Signing

Ed25519 signing for submission attribution. Project-local config at `.config/minegraph/`.

```bash
minegraph init                                     # create config directory
minegraph keygen --name "my-desktop"               # generate signing keypair
minegraph whoami                                   # show current identity
minegraph register-key --server http://localhost:3001  # register with server
minegraph config show                              # view all settings
```

Workers auto-detect the signing key from `.config/minegraph/key.json` and sign
submissions. The `--commit-hash` flag attaches a git commit for provenance.

```bash
./run search --release --k 5 --ell 5 --n 25 --commit-hash $(git rev-parse --short HEAD)
```

Server verifies signatures against registered keys. Sig status: `verified`,
`unregistered` (key not registered), `invalid` (bad signature), `anonymous`
(no key provided). Web app shows key_id or "anon" on leaderboard entries.

**API endpoints:**
- `POST /api/keys` — register a public key with display_name and github_repo
- `GET /api/keys/{key_id}` — look up identity info

## Phase Status

Phases 0–6 complete. Current focus: strategy optimization via experiment loop.
Phase 6 (ed25519 identity) implemented — see `docs/SIGNING_DESIGN.md`.
