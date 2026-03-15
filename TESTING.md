# RamseyNet — Testing Guide

Interactive walkthrough for testing the web application and API.

---

## 1. Prerequisites

- Rust stable + pnpm/Node.js (see README)
- **curl** and optionally **jq**
- A modern web browser

---

## 2. Setup

### 2a. Build and verify tests pass

```
./run ci
```

You should see `=== CI passed! ===` at the end.

### 2b. Start the API server

Open a terminal for the server (logs saved to `logs/`):

```
./run server-log
```

Server starts on **http://localhost:3001**.

### 2c. Start the web dev server

In a second terminal:

```
./run web-dev
```

Web app starts on **http://localhost:5173**.

### 2d. Seed test data

In a third terminal:

```
./run seed
```

This submits four graphs to the leaderboard system:
- **C5** → R(3,3) n=5 — accepted, admitted to leaderboard
- **Wagner graph** → R(3,4) n=8 — accepted, admitted to leaderboard
- **K5** → R(3,3) n=5 — rejected (clique_found, witness [0,1,2])
- **E5 (empty)** → R(3,3) n=5 — rejected (independent_set_found, witness [0,1,2])

### 2e. Server logs

```
ls -la logs/
```

---

## 3. Feature Walkthrough

Open **http://localhost:5173** in your browser.

### 3.1 Homepage

- [ ] RamseyNet title renders with purple gradient
- [ ] Green status badge shows `RamseyNet v0.1.0 — ok`
- [ ] Two navigation cards: Leaderboards, Submit
- [ ] Live Events panel shows a green connection dot
- [ ] Events from the seed script appear (graph.submitted, graph.verified, leaderboard.admitted)
- [ ] Events with CIDs are clickable links to submission detail pages
- [ ] No favicon 404 in the network tab (SVG K5 favicon loads)

### 3.2 Leaderboards (`/leaderboards`)

Click **"Browse leaderboards"** or the **Leaderboards** nav link.

- [ ] (K,L) pairs grouped: R(3,3), R(3,4), R(4,4)
- [ ] Each pair shows available n values with entry counts
- [ ] Click an n-value chip to navigate to the leaderboard detail

### 3.3 Leaderboard Detail (`/leaderboards/3/3/5`)

Click the **n=5** chip under R(3,3).

- [ ] Header: `R(3,3) n = 5` with entry count (e.g., "Showing 1-50 of 500 entries")
- [ ] Ranked table with columns: #, Graph, CID, C_max, C_min, Gap (Goodman), |Aut|, Admitted
- [ ] Gap column: 0 values highlighted green
- [ ] Top graph visualization: Matrix View + Circle Layout (only on page 1)
- [ ] Pagination controls when total > 50 (first/prev/page numbers/next/last)
- [ ] Click a CID → navigates to submission detail
- [ ] CSV export button downloads current page data

### 3.4 Submission Detail (`/submissions/[cid]`)

Click a CID link on any leaderboard entry or event feed.

- [ ] Back button navigates to previous page
- [ ] Full CID displayed in monospace header
- [ ] R(k,l) link → navigates to `/leaderboards/[k]/[l]/[n]`
- [ ] Graph size (n) displayed
- [ ] Verdict badge: green ACCEPTED or red REJECTED
- [ ] Rank badge shown when submission is on the leaderboard
- [ ] Reason text (if rejected)
- [ ] Witness vertices (if present)
- [ ] **Score Details** section with grid: omega, alpha, C_omega, C_alpha, Triangles (G), Triangles (complement), Goodman #, Goodman minimum, Goodman gap (0 highlighted green), |Aut(G)|
- [ ] Matrix View + Circle Layout side-by-side (with witness overlay for rejected graphs)
- [ ] Submitted and Verified timestamps

### 3.5 Submit Page (`/submit`)

Click the **Submit** nav link.

- [ ] K, L, N number inputs
- [ ] RGXF JSON textarea with placeholder

#### Test A: Accepted graph

1. Enter K=4, L=4, N=5
2. Paste: `{"n": 5, "encoding": "utri_b64_v1", "bits_b64": "mUA="}`
3. Verify: Live matrix preview appears (N auto-fills from RGXF)
4. Click **Submit Graph**
5. Verify: Green result with `ACCEPTED` and admission to leaderboard

#### Test B: Rejected graph

1. Enter K=3, L=3, N=4
2. Paste: `{"n": 4, "encoding": "utri_b64_v1", "bits_b64": "/A=="}`
3. Click **Submit Graph**
4. Verify: Red result with `REJECTED`, reason `clique_found`, witness `[0, 1, 2, 3]`

#### Test C: Invalid input

1. Type `not valid json` → "Invalid JSON" error, submit disabled
2. Type `{"n": 5}` → "Missing required fields" error

### 3.6 Navigation

- [ ] RamseyNet logo → homepage
- [ ] Leaderboards / Submit nav links work
- [ ] Browser back/forward works (SPA routing)
- [ ] Direct URL access works (e.g., `/leaderboards/3/3/5`)

### 3.7 WebSocket Reconnect

1. Stop the API server (Ctrl+C)
2. Verify: Connection dot turns red
3. Restart: `./run server-log`
4. Verify: Dot turns green within a few seconds

---

## 4. API Testing with curl

```bash
# Health check
curl -s localhost:3001/api/health | jq .

# List all leaderboards
curl -s localhost:3001/api/leaderboards | jq .

# List n values for R(3,3)
curl -s localhost:3001/api/leaderboards/3/3 | jq .

# Paginated leaderboard for R(3,3) n=5 (first page)
curl -s 'localhost:3001/api/leaderboards/3/3/5?offset=0&limit=50' | jq .

# Paginated leaderboard (page 2)
curl -s 'localhost:3001/api/leaderboards/3/3/5?offset=50&limit=50' | jq .

# Admission threshold
curl -s localhost:3001/api/leaderboards/3/3/5/threshold | jq .

# RGXF graphs for leaderboard entries (top 10)
curl -s 'localhost:3001/api/leaderboards/3/3/5/graphs?limit=10' | jq .

# Incremental CID sync (all CIDs)
curl -s localhost:3001/api/leaderboards/3/3/5/cids | jq .

# Incremental CID sync (only since a timestamp)
curl -s 'localhost:3001/api/leaderboards/3/3/5/cids?since=2026-03-14T00:00:00Z' | jq .

# Submission detail (replace CID with a real one from leaderboard)
curl -s localhost:3001/api/submissions/<cid> | jq .

# Stateless verification
curl -s -X POST localhost:3001/api/verify \
  -H "Content-Type: application/json" \
  -d '{"oras_version":"ovwc-1","k":3,"ell":3,"graph":{"n":5,"encoding":"utri_b64_v1","bits_b64":"mUA="},"want_cid":true}' | jq .

# Submit a graph
curl -s -X POST localhost:3001/api/submit \
  -H "Content-Type: application/json" \
  -d '{"k":4,"ell":4,"n":5,"graph":{"n":5,"encoding":"utri_b64_v1","bits_b64":"mUA="}}' | jq .

# WebSocket event stream (requires wscat: npm install -g wscat)
wscat -c ws://localhost:3001/api/events
```

---

## 5. Test Graphs Reference

From `test-vectors/small_graphs.json`:

| Graph | n | bits_b64 | R(3,3) | R(3,4) |
|-------|---|----------|--------|--------|
| C5 (5-cycle) | 5 | `mUA=` | accepted | accepted |
| K5 (complete) | 5 | `/8A=` | rejected | rejected |
| E5 (empty) | 5 | `AAA=` | rejected | rejected |
| Petersen | 10 | `mEREiCzQ` | — | rejected |
| Wagner | 8 | `kySmUA==` | — | accepted |
| K4 (complete) | 4 | `/A==` | rejected | — |

RGXF JSON format: `{"n": 5, "encoding": "utri_b64_v1", "bits_b64": "mUA="}`

---

## 6. Cleanup

To start fresh:

```
rm -f ramseynet.db ramseynet.db-wal ramseynet.db-shm
```

Then restart the server and re-seed.

---

## 7. Search Worker Testing

### 7a. Prerequisites

Start the API server and seed data (sections 2b + 2d above).

### 7b. Basic run

```bash
./run search --k 3 --ell 3 --n 5 --strategy greedy --max-iters 1000
```

Expected: worker connects, searches for valid R(3,3) graphs on n=5 vertices, submits competitive graphs to the leaderboard, and reports admission results. Press Ctrl+C to stop.

### 7c. Strategy-specific runs

```bash
# Local search with tabu
./run search --k 3 --ell 3 --n 5 --strategy local --max-iters 10000

# Simulated annealing
./run search --k 3 --ell 3 --n 5 --strategy annealing --max-iters 50000

# Tree search (beam search)
./run search --k 3 --ell 3 --n 5 --strategy tree

# All strategies (default)
./run search --k 3 --ell 3 --n 5
```

### 7d. Larger searches

```bash
# R(3,4) n=8 — Wagner graph is the classic solution
./run search --k 3 --ell 4 --n 8 --strategy greedy

# R(4,4) n=17 — Paley graph is the classic solution
./run search --k 4 --ell 4 --n 17 --strategy all
```

### 7e. Leaderboard seeding and sampling

```bash
# Seed from leaderboard with moderate exploration (bias 0.3 = more uniform)
./run search --k 4 --ell 4 --n 17 --init leaderboard --sample-bias 0.3

# Seed from leaderboard with top-heavy sampling (bias 0.9 = mostly top graphs)
./run search --k 4 --ell 4 --n 17 --init leaderboard --sample-bias 0.9

# Larger seed pool with custom collector capacity
./run search --k 4 --ell 4 --n 17 --init leaderboard --leaderboard-sample-size 200 --collector-capacity 2000
```

- [ ] Server logs show varied `offset=` values (not always 0) when `--sample-bias < 1.0`
- [ ] Worker logs show `refreshed leaderboard seed pool count=N offset=M`
- [ ] Worker logs show `synced leaderboard CIDs` with incremental `new_cids` after first round

### 7f. Offline mode

```bash
# No server needed — search with local viz only
./run search --k 3 --ell 3 --n 5 --offline --port 8080
```

Open http://localhost:8080 to see the search visualization dashboard.

### 7g. Verify submissions appear in the UI

After running the search worker:
- [ ] `/leaderboards/3/3/5` shows new entries in the ranked table
- [ ] Event feed on homepage shows `graph.submitted` and `leaderboard.admitted` events
- [ ] Submission detail pages load correctly for worker-submitted graphs
- [ ] Leaderboard list at `/leaderboards` reflects updated entry counts

### 7h. Graceful shutdown

1. Start: `./run search --k 3 --ell 3 --n 5`
2. Press Ctrl+C
3. Verify: Worker logs `Ctrl+C received, shutting down...` and exits cleanly

### 7i. Error handling

```bash
# Server not running — should fail with connection error
./run search --k 3 --ell 3 --n 5 --server http://localhost:9999

# K and L are required
./run search --k 3 --n 5  # should fail with missing --ell
```

---

## 8. Known Limitations

- No authentication or signing (Phase 6)
- No P2P networking (Phase 6)
- Database is local SQLite — persists across restarts but is per-environment
- WebSocket close/reconnect messages in browser console on server restart are expected
- Search worker has no identity — all submissions are anonymous until Phase 6
