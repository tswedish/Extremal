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

1. **Plateau detection**: admission rate < 1/5min for 3+ consecutive observations
2. **Worker comparison**: which config has highest admit rate per minute?
3. **Score frontier**: compare snapshots — is top score improving?
4. **Round time budget**: polish_max_steps dominates round time when many valid graphs are found
5. **Score history trend**: query `/api/leaderboards/25/history` to see if `avg_gap` and `best_gap` are still improving or have plateaued. A flat `avg_gap` over several snapshots signals the current strategy has hit its ceiling — time to signal the orchestrator for research.

### Lessons learned
- **First round is always slow** (~2min) because it starts from Paley seed with 100K iters. Subsequent rounds are faster (seeded from leaderboard).
- **polish_max_steps=100 is a good default**. 500 was too slow for first round. Can increase to 200-500 via runtime config adjustment after warmup.
- **Round 1 submissions fail if key isn't registered** — loop.sh and agent-fleet.sh handle this automatically.
- **Polish debug logs require RUST_LOG=debug** — at info level, use worker HTTP API `/api/status` for metrics instead.
- **Dashboard shows only 1 worker per worker_id** — always use unique worker_ids in metadata.
- **Threshold gating is aggressive on full leaderboards** — hundreds of thousands of graphs skipped. This is normal.
- **Use direct HTTP API for config changes, NOT the CLI `workers set` command** — the CLI discovers workers via relay and times out. Instead: `curl -sf -X POST http://localhost:$PORT/api/config -H "Content-Type: application/json" -d '{"param": value}'`
- **Worker API ports** are in the agent-status.sh output, or query: `curl -sf http://localhost:4000/api/workers`
- **beam_width=150 + noise_flips=2 + sample_bias=0.4** was the clear winner in production experiments (19 admits/min vs 0.2-6.7 for other configs).

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

```bash
# 1. Find worker API ports from dashboard relay
curl -sf http://localhost:4000/api/workers | python3 -c "
import json, sys
for w in json.load(sys.stdin)['workers']:
    print(f'{w[\"worker_id\"]}: {w[\"api_addr\"]}')"

# 2. Adjust params via direct HTTP POST (takes effect next round)
curl -sf -X POST http://localhost:$PORT/api/config \
  -H "Content-Type: application/json" \
  -d '{"beam_width": 150, "noise_flips": 2, "sample_bias": 0.4}'

# 3. Check current config
curl -sf http://localhost:$PORT/api/config | python3 -m json.tool

# 4. Pause/resume/stop
curl -sf -X POST http://localhost:$PORT/api/pause
curl -sf -X POST http://localhost:$PORT/api/resume
curl -sf -X POST http://localhost:$PORT/api/stop
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

```
## Experiment Report [HH:MM]

**Leaderboard**: X entries for n=N, top 4c=(X,Y) 3c=(X,Y)
**Fleet**: W workers, commit HASH, uptime Xm
**Admission rate**: Y/min (was Z/min last check)

### Per-Worker
| Worker | Rounds | Disc | Admitted | Rate/min | Round ms |
|--------|--------|------|----------|----------|----------|

### Actions Taken Since Last Report
- [timestamp] [action] [result]

### Key Finding
[Finding with evidence]

### Proposed Next Action
[Description]. Proceed? (y/n)
```

## Journal Entry Format

Append to `experiments/agent/journal.md`:
```
### [YYYY-MM-DD HH:MM] [ACTION_TYPE]
**Context**: [metrics observed]
**Decision**: [what and why]
**Action**: [command run]
**Result**: [measured after next observation]
```
