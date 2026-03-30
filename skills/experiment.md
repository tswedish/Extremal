# Experiment Agent

Autonomous experiment management for Extremal graph search workers.
This skill is loaded as system context by `scripts/loop.sh` and can also be invoked interactively.

## Invocation

**Autonomous loop** (preferred): `./run agent-loop` or `./scripts/loop.sh`
- Launches a worker fleet, then runs Claude in a cycle every N minutes
- Each cycle: observe fleet status, analyze, decide on adjustments, act, report
- This skill is loaded via `--append-system-prompt-file` so you have the full protocol

**Interactive**: User says `/experiment adjust` (or status/stop/report) in a Claude session.

**Modes** (referenced in loop prompt or interactive sessions):
- **adjust** - Single observe-decide-act cycle (the main loop action)
- **status** - One-shot observation and analysis, no changes
- **stop** - Graceful fleet shutdown with final report
- **report** - Generate findings summary from journal

## Scripts

Four helper scripts handle the mechanical work:

| Script | Purpose | Usage |
|--------|---------|-------|
| `./scripts/loop.sh` | Full agent loop (fleet + observe cycles) | `./scripts/loop.sh --workers 4 --interval 5m` |
| `./scripts/agent-fleet.sh` | Launch fleet with full hygiene | `./scripts/agent-fleet.sh --workers 4 --n 25 --polish 100` |
| `./scripts/agent-status.sh` | Formatted status report | `./scripts/agent-status.sh [LOG_DIR]` |
| `./scripts/agent-snapshot.sh` | Leaderboard snapshot | `./scripts/agent-snapshot.sh [N] [SERVER_URL]` |

### agent-fleet.sh handles:
- Release build
- Signing key generation + server registration (idempotent)
- Unique worker_ids from diverse config presets (wide-a/b, focused, deep, explore...)
- Commit hash + timestamp in metadata
- config.json with all params + PIDs
- Signal trapping for graceful shutdown
- Optional `--duration 30m` for timed runs

### agent-status.sh provides:
- Leaderboard top-3 with score breakdown
- Per-worker metrics via HTTP API (round, discoveries, admitted, rate, round_ms)
- Recent round log lines with threshold skip counts
- Works with any log directory (defaults to most recent)

## State Files

All state persists in `experiments/agent/`:
- `strategies.json` - Strategy registry: validated configs, code changes, and ideas to try
- `journal.md` - Append-only decision log with timestamps
- `findings.json` - Validated parameter sensitivities and strategy rankings
- `state.json` - Current session config
- `snapshots/` - Leaderboard JSON snapshots (via `agent-snapshot.sh`)
- `inbox/` - Operator messages (see below)

### Operator Inbox (`experiments/agent/inbox/`)

The operator can drop `.md` files here to send you messages between cycles. If your prompt
includes an "Operator Messages" section, address those messages FIRST in your response and
actions. The orchestrator moves processed files to `inbox/processed/` automatically.

**IMPORTANT: Inbox messages are one-shot.** You only see each message once — future cycles
have no memory of it. When you receive an inbox message:
1. **Read and understand** the operator's intent
2. **Synthesize** your own judgment — do you agree? What's actionable? What needs nuance?
3. **Write a finding** to `experiments/agent/findings.json` capturing your synthesis (not
   a copy of the message, but your informed take on it)
4. **Act** on whatever is immediately actionable
5. **Report** in your cycle output what you learned and what you persisted

### Findings (`experiments/agent/findings.json`)

This is your persistent memory across cycles. Read it at the start of each cycle to maintain
context. Write to it when you learn something that should inform future decisions.

**When to read**: Every cycle. Skim for entries relevant to current fleet state.
**When to write**: After inbox messages, after experiments confirm/refute a hypothesis,
after discovering a surprising result.
**What to write**: Synthesized knowledge, not raw data. "beam_width=200 outperformed 100
by 3x on discovery rate in the first 20 minutes" — not a copy of log lines.

### Strategy Registry (`strategies.json`)

The registry is shared between the experiment skill and the strategy-research skill:
- **strategies[]**: Validated configs and code changes with performance metrics
- **ideas[]**: Proposed improvements with priority and effort estimates
- When the experiment agent tests a strategy marked `untested`, update its `status` and `metrics`
- When experiments plateau, the orchestrator triggers strategy-research to implement new ideas

## Observe Phase

**Primary method**: Run `./scripts/agent-status.sh` — this queries worker HTTP APIs directly (more reliable than log parsing) and formats everything.

**For deeper analysis**, also query:
```bash
# Admission threshold (hex score bytes)
curl -sf https://api.extremal.online/api/leaderboards/25/threshold

# Specific worker config (shows adjustable params)
curl -sf http://localhost:$PORT/api/config

# Full leaderboard snapshot
./scripts/agent-snapshot.sh 25

# Score history — leaderboard quality over time (best/avg/worst gap, aut)
curl -sf https://api.extremal.online/api/leaderboards/25/history?limit=20
# Since a specific time:
curl -sf "https://api.extremal.online/api/leaderboards/25/history?since=2026-03-26T00:00:00Z"

# Submission history with metadata (worker_id, commit, strategy params)
curl -sf https://api.extremal.online/api/keys/da8d7f22fe695511/submissions?limit=50

# Individual submission detail
curl -sf https://api.extremal.online/api/submissions/{cid}

# Export leaderboard as CSV or graph6 (for bulk analysis)
curl -sf https://api.extremal.online/api/leaderboards/25/export/csv
curl -sf https://api.extremal.online/api/leaderboards/25/export
```

### Key metrics to extract
- **Admission rate**: `total_admitted / elapsed_minutes` per worker
- **Discovery efficiency**: `total_admitted / total_discoveries` (what % of valid graphs beat the threshold)
- **Round time trend**: is `last_round_ms` stable, improving, or degrading?
- **Threshold skip count**: `skip_thr` in round logs — higher = more competitive leaderboard

## Analyze Phase

1. **Plateau detection**: admission rate = 0 for **1-2 hours** (not minutes). On saturated leaderboards, progress is slow — be patient
2. **Worker comparison**: which config has highest admit rate per minute?
3. **Score frontier**: compare snapshots — is top score improving?
4. **Round time budget**: polish_max_steps dominates round time when many valid graphs are found
5. **Score history trend**: query `/api/leaderboards/25/history` to see if `avg_gap` and `best_gap` are still improving or have plateaued. A flat `avg_gap` over several snapshots signals the current strategy has hit its ceiling — time to signal the orchestrator for research.

### Lessons learned

**Patience and scale:**
- **Do NOT declare plateau after 5-15 minutes.** On saturated leaderboards, progress takes hours or days. Wait at least 1-2 hours of zero admits before concluding the algorithm is stuck.
- **Any admission on a full leaderboard means scores are improving.** Even 1 admit/hour is progress — the new graph necessarily beat an existing entry. Track total admissions over time, not just rate.
- **Scale workers up to 16** before concluding the algorithm can't make progress. It may be a throughput issue, not an algorithmic ceiling. Check CPU load with `top` or `htop` and add workers if there's headroom.
- **An nvidia GPU is available locally** — consider GPU acceleration for compute-intensive operations (clique counting, candidate evaluation).

**Richer metrics — don't fixate on avg_gap alone:**
- Track average 4-clique counts across the leaderboard (query history endpoint)
- Watch the distribution shift: fewer high-4-clique graphs being displaced = progress
- Score history (`/api/leaderboards/25/history`) shows `best_gap`, `avg_gap`, `best_aut`, `avg_aut` over time
- Compare score distributions between snapshots, not just single summary stats

**Operational:**
- **First round is always slow** (~2min) because it starts from Paley seed with 100K iters. Subsequent rounds are faster (seeded from leaderboard).
- **polish_max_steps=100 is a good default**. 500 was too slow for first round. Can increase to 200-500 via runtime config adjustment after warmup.
- **Round 1 submissions fail if key isn't registered** — loop.sh and agent-fleet.sh handle this automatically.
- **Polish debug logs require RUST_LOG=debug** — at info level, use worker HTTP API `/api/status` for metrics instead.
- **Dashboard shows only 1 worker per worker_id** — always use unique worker_ids in metadata.
- **Threshold gating is aggressive on full leaderboards** — hundreds of thousands of graphs skipped. This is normal and expected.
- **Use direct HTTP API for config changes, NOT the CLI `workers set` command** — the CLI discovers workers via relay and times out. Instead: `curl -sf --max-time 10 -X POST http://localhost:$PORT/api/config -H "Content-Type: application/json" -d '{"param": value}'`
- **ALWAYS use `--max-time 10` on ALL curl calls to worker APIs** — config/status/pause/resume endpoints block until the current round finishes, which can be 5-20+ minutes with ILS. The config is queued server-side even if curl times out, so do NOT retry or wait.
- **Worker API ports** are in the agent-status.sh output, or query: `curl -sf http://localhost:4000/api/workers`
- **beam_width=150 + noise_flips=2 + sample_bias=0.4** was the clear winner in production experiments (19 admits/min vs 0.2-6.7 for other configs).

**Tuning (post polish-optimization 2026-03-30):**
- Rounds are now ~170x faster due to polish capping (5 walks/depth) and deferred canonical_form.
- **Increase search params** to maximize productive search vs per-round overhead: `max_iters=500000`, `beam_width=150`, `max_depth=15`. At 100k iters, only ~35% of wall time is actual search.
- `max_polish_per_depth` defaults to 5 (adjustable at runtime). Set to 0 for unlimited (old behavior).
- Post-search scoring is capped at 200 discoveries per round to avoid spending more time scoring than searching.

**Local convergence benchmarking:**
- Each worker's local discovery history (best graphs found over time) is a useful optimization signal independent of production leaderboard saturation.
- **Mini-experiments**: Compare how fast different configs improve their local best score in the first 10-20 minutes. Faster local convergence = more efficient search.
- **Track per worker**: time-to-first-valid, local best score at 5/10/30 min marks, discovery rate curve over time.
- **Use for A/B testing**: Run 2 workers with different configs, compare local convergence curves. The steeper curve is the better searcher.
- **Caveat**: Local convergence measures search efficiency, not absolute quality. Always validate that local improvements translate to production admits.
- Log mini-experiment findings to journal with tag "LOCAL-BENCHMARK".

## Decide Phase

### Autonomous Actions
| Condition | Action | How |
|-----------|--------|-----|
| Plateau + homogeneous fleet | Diversify configs | `workers set $ID beam_width=200 noise_flips=2` |
| Plateau + already diverse | Increase noise, lower bias | `workers set $ID noise_flips=3 sample_bias=0.3` |
| One worker 3x better | Migrate worst to winning config | Copy params from best worker |
| High discovery, low admission | Increase polish depth | `workers set $ID polish_max_steps=200` |
| Worker crashed/stopped | Log and report | Check PID file, restart if needed |
| Round times stable, fleet warmed up | Increase polish | `workers set $ID polish_max_steps=200` |

### Requires User Approval
| Condition | Action |
|-----------|--------|
| Untested strategy or major param change | Propose A/B experiment |
| Fleet restart needed | Propose plan with rationale |
| Want to stop all workers | Confirm first |

### Always Report
| Condition | Action |
|-----------|--------|
| 30+ min since last check-in | Summary report |
| New best score found | Immediate notification |
| Experiment cycle complete | Before/after snapshot comparison |

## Act Phase

### Adjust running workers (via direct HTTP API — preferred)

IMPORTANT: Always use the direct worker HTTP API, not the CLI `workers set` command (it times out).

CRITICAL: Worker config POST endpoints BLOCK until the current round finishes (can be 5-20+ minutes with ILS polish). ALWAYS use `--max-time 10` on curl commands to avoid blocking. The config change is queued on the server side even if curl times out — you do NOT need to wait for a response.

```bash
# 1. Find worker API ports from dashboard relay
curl -sf --max-time 5 http://localhost:4000/api/workers | python3 -c "
import json, sys
for w in json.load(sys.stdin)['workers']:
    print(f'{w[\"worker_id\"]}: {w[\"api_addr\"]}')"

# 2. Adjust params via direct HTTP POST (takes effect next round)
# ALWAYS use --max-time 10 — the request blocks until round ends!
curl -sf --max-time 10 -X POST http://localhost:$PORT/api/config \
  -H "Content-Type: application/json" \
  -d '{"beam_width": 150, "noise_flips": 2, "sample_bias": 0.4}'
# Config is queued even if this times out — do NOT retry.

# 3. Check current config (also blocks until round boundary)
curl -sf --max-time 10 http://localhost:$PORT/api/config | python3 -m json.tool

# 4. Pause/resume/stop (also blocks)
curl -sf --max-time 10 -X POST http://localhost:$PORT/api/pause
curl -sf --max-time 10 -X POST http://localhost:$PORT/api/resume
curl -sf --max-time 10 -X POST http://localhost:$PORT/api/stop
```

### Take snapshot
```bash
./scripts/agent-snapshot.sh 25
```

### Launch additional workers
```bash
# Add 2 more workers to existing fleet
./scripts/agent-fleet.sh --workers 2 --n 25 --polish 200
```

## Safety Rules

1. **Minimum observation window**: Never change a worker's config more than once per 5 minutes
2. **Control group**: Always keep at least 1 worker on known-best config unchanged
3. **Graduated changes**: Try on 1 worker first, migrate after 10 min if positive
4. **Prior findings**: Check `experiments/agent/findings.json` before trying configs
5. **No fleet-wide stop without approval**

## Report Format

Your stdout output is displayed directly in the operator's terminal. Make it engaging and
insightful — the operator is watching to understand what the search is doing and why.

```
## Cycle N/M [HH:MM]

**Leaderboard**: X entries, top 4c=(X,Y) gap=Z, trend [improving/flat/degrading]
**Fleet**: W workers, R rounds, D discoveries, A admissions, best: [worker] ([why])
**Threshold**: 4c=(X,Y) — [how close are we / what it would take to break through]

### What I'm Seeing
[2-3 sentences of genuine analysis. What patterns are emerging across workers?
Which configs produce results and which don't? Is the landscape being explored
effectively? Any signs of convergence or exhaustion? What does the skip_thr
count tell us about threshold saturation?]

### Strategy Thinking
[Your current theory about what will produce improvements. What hypothesis are
you testing with the current fleet composition? What evidence would confirm or
refute it? Reference prior journal findings if relevant.]

### Actions Taken
- [Specific change with reasoning], or
- None — [why observing is the right call this cycle]

### Next Cycle
[Concrete things to watch. What metric change would trigger action?]
```

Keep the analysis genuine — don't pad with boilerplate. If nothing interesting happened,
say that honestly and explain what conditions you're waiting for.

## Journal Entry Format

Append to `experiments/agent/journal.md`:
```
### [YYYY-MM-DD HH:MM] [ACTION_TYPE]
**Context**: [metrics observed]
**Decision**: [what and why]
**Action**: [command run]
**Result**: [measured after next observation]
```
