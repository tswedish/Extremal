# Strategy Research Agent

Research, design, and implement new search strategies or optimizations for Extremal.
This skill is used by the orchestrator when the system decides research is more valuable than running experiments.

## CRITICAL: Complexity Discipline

**Before implementing ANYTHING:**

1. **Check untested strategies first.** Read `experiments/agent/strategies.json`. If there
   are already untested strategies, DO NOT add more. Return immediately and tell the
   orchestrator to run experiments on existing untested strategies instead.
2. **Verify the baseline had a fair run.** tree2+ILS with deep polish (polish_max_steps=500,
   polish_ils_restarts=3+, max_iters=500000) gets admissions when run for 2+ hours. Short
   runs or runs with wrong configs (e.g., capped polish) do NOT prove the algorithm is exhausted.
3. **ONE strategy per research cycle.** Implement one, commit, let experiments test it.
   Batching 10 strategies in one session creates untested complexity.
4. **Every strategy must be A/B tested.** Run new strategy alongside tree2 baseline, compare
   local convergence rates. If it doesn't beat baseline after 30+ minutes, mark "ineffective."
5. **Roll back failures.** Check findings.json for strategies already marked ineffective.
   Don't re-implement failed approaches with minor variations.

## Goal

Improve the quality of graphs found by workers, measured by:
1. Better leaderboard scores (fewer 4-cliques, better triangle balance, higher symmetry)
2. Higher admission rate (more graphs beating the threshold)
3. Faster discovery of valid graphs
4. Can target n=25 or n=35 (n=35 may have less saturated leaderboard for clearer signal)

## Inputs

Before starting, read these files to understand current state:

1. `experiments/agent/strategies.json` — Registry of known strategies, their performance, and ideas to try
2. `experiments/agent/findings.json` — Validated experimental findings
3. `experiments/agent/journal.md` — Recent experiment history and outcomes
4. `CLAUDE.md` — Full architecture and scoring system docs

### Server data for analysis

The server stores a full score history with submission metadata. Use these APIs to analyze what's working:

```bash
# Leaderboard with scores (graph6, histogram, goodman_gap, aut_order)
curl -sf https://api.extremal.online/api/leaderboards/25?limit=50

# Score history — time-series of leaderboard quality over time
# Returns: t, count, best_gap, worst_gap, median_gap, avg_gap, best_aut, avg_aut
curl -sf https://api.extremal.online/api/leaderboards/25/history?limit=50
# Filter by date:
curl -sf "https://api.extremal.online/api/leaderboards/25/history?since=2026-03-26T00:00:00Z"

# Submission history for our key (includes metadata: worker_id, commit, strategy params)
curl -sf https://api.extremal.online/api/keys/da8d7f22fe695511/submissions?limit=100

# Individual submission detail (full score breakdown)
curl -sf https://api.extremal.online/api/submissions/{cid}

# Export full leaderboard as graph6 (for bulk analysis)
curl -sf https://api.extremal.online/api/leaderboards/25/export

# Export as CSV (rank, cid, graph6, goodman_gap, aut_order, key_id, admitted_at)
curl -sf https://api.extremal.online/api/leaderboards/25/export/csv

# Score a graph locally without submitting
cargo run -p extremal-cli -- score --n 25 --graph6 '<graph6_string>'
```

**Score history** is especially valuable for research. It shows how leaderboard quality evolves over time:
- `best_gap` / `avg_gap` / `worst_gap` — Goodman gap trend (lower = better triangle balance)
- `best_aut` / `avg_aut` — automorphism order trend (higher = more symmetric)
- Plateaus in these metrics signal that the current strategy has hit its ceiling
- Improvements after a code change confirm the change had impact
- Use `?since=` to compare before/after a specific commit or experiment

**Submission metadata** is attached to every graph submitted by workers. Currently includes `worker_id`, `commit`, and `started`. This is valuable for:
- **Attribution**: Which worker config produced which top-scoring graphs?
- **A/B analysis**: Compare score distributions between commits or worker configs
- **Provenance**: Trace a top graph back to the strategy/params that found it

Consider enriching metadata in future strategies to include:
- `strategy_id` — which strategy found the graph (tree2, tabu, etc.)
- `polish_steps_used` — how many polish steps were taken for this graph
- `seed_cid` — CID of the leaderboard graph used as seed (tracks lineage)
- `round` — which round of the worker produced this graph
- `violations_at_discovery` — was the graph found with 0 violations, or polished to 0?

This lets the research agent ask questions like "do graphs seeded from top-10 entries produce better offspring?" or "does the tabu strategy find structurally different graphs than tree2?"

## Process

### Compute resources

- **CPU**: Up to 16 parallel workers on the local machine. Check `nproc` and `htop` for headroom.
- **GPU**: An nvidia GPU is available locally. Consider GPU-accelerated clique counting, candidate evaluation, or batch scoring. CUDA/OpenCL FFI or wgpu compute shaders are options.
- **Scaling**: More workers with good configs often beats algorithmic cleverness. Don't overlook throughput.

### 1. Assess

Read the registry's `ideas` list and `strategies` list. Ask:
- Which ideas are highest priority and lowest effort?
- What's the current performance ceiling? (check top leaderboard scores via score history)
- Are there diminishing returns on current approaches? (check recent journal — but note experiments need hours/days, not minutes, to show results on competitive leaderboards)
- What's the biggest bottleneck: finding valid graphs, or scoring quality of valid graphs?
- Could raw compute throughput (more workers) solve the problem before algorithmic changes?

### 2. Choose

Pick ONE idea to implement. Prefer:
- High priority + low effort first (quick wins)
- Ideas that address the current bottleneck
- Ideas that are complementary to existing validated strategies (not replacements)

If no existing idea fits, propose a new one based on:
- Patterns in the codebase (read strategy implementations)
- The scoring system structure (what actually differentiates top graphs)
- Mathematical properties of Ramsey graphs

### 3. Implement

Write the code. Key files:

| What | Where |
|------|-------|
| New strategy | `crates/extremal-strategies/src/<name>.rs` |
| Strategy registration | `crates/extremal-strategies/src/lib.rs` |
| Config presets | `experiments/agent/strategies.json` (add to strategies list) |
| Engine changes | `crates/extremal-worker-core/src/engine.rs` |
| Polish improvements | `crates/extremal-strategies/src/polish.rs` |
| Worker CLI flags | `crates/extremal-worker/src/main.rs` |

Follow existing patterns:
- Implement `SearchStrategy` trait (see `tree2.rs` or `tabu.rs`)
- Use `violation_delta` for incremental scoring
- Use `canonical_form` + CID for dedup
- Report discoveries via `observer.on_discovery()`
- Add tests (at minimum: R(3,3)/n=5 sanity check)

### 4. Validate

```bash
./run ci    # Must pass: fmt + clippy + all tests
```

If adding a new strategy, also run the experiment harness:
```bash
cargo run -p extremal-experiments --release -- compare --n 25 --budget 100000 --seeds 5
```

### 5. Commit

Commit on the current branch (do NOT create a new branch).
**CRITICAL: Respect .gitignore.** The `experiments/` directory is gitignored. NEVER
`git add` files under `experiments/`. Only commit source code changes in `crates/` and `scripts/`.
```bash
git add crates/ scripts/    # ONLY source code, never experiments/
git commit -m "feat: <description of strategy change>"
```

### 6. Update Registry

Update `experiments/agent/strategies.json` (this file is NOT tracked in git — it's local state):
- If new strategy: add to `strategies` list with `status: "untested"`
- If new config preset: add to `strategies` list with `status: "untested"`
- Move the implemented idea from `ideas` to `strategies`
- Add any new ideas discovered during implementation to `ideas`

### 7. Report

Output a summary:
```
## Strategy Research Report

**Implemented**: [id] — [description]
**Commit**: [hash]
**Files changed**: [list]
**Status**: untested — ready for experiment validation
**Expected impact**: [what should improve and why]
**How to test**: [specific experiment config or fleet params]
```

## Available Analysis Tools

Use these scripts to inform your research:

```bash
# Trace what commit/config produced top leaderboard entries
./scripts/agent-provenance.sh 25        # n=25, top 20
./scripts/agent-provenance.sh 35 50     # n=35, top 50

# A/B test a change before committing to full fleet
./scripts/agent-ab-test.sh --duration 10m \
  --a "--beam-width 150 --noise-flips 2" \
  --b "--beam-width 150 --noise-flips 4 --polish-max-steps 500"

# Check fleet status
./scripts/agent-status.sh

# Leaderboard snapshot
./scripts/agent-snapshot.sh 25
```

## Guidelines

- **One change per research cycle**. Don't try to do everything at once.
- **Smallest viable change**. A config preset is easier than a new strategy. An engine tweak is easier than a new algorithm.
- **Build on what works**. The tree2 + deep polish pipeline is validated. Extend it, don't replace it.
- **Measure before and after**. Every change should have a clear metric to evaluate.
- **Stay on current branch**. The orchestrator expects commits on the active branch.
- **Don't break existing tests**. `./run ci` must pass after your changes.

## Code Hygiene

**Keep the codebase close to what works.** The strategies crate should contain:
- **Core strategies** that have produced production admissions (tree2, tabu, polish)
- **Promising strategies** actively being tested
- NOT a graveyard of untested ideas

**When adding a strategy:**
1. Implement it, add tests, commit
2. The experiment phase tests it in production
3. If it produces admissions → keep it, document results
4. If it doesn't after fair testing → **remove the code**, document what was tried in
   findings.json. Don't leave 600-line files around "just in case."

**When removing a strategy:**
1. Document in findings.json: what it did, why it was removed, what might make it worth revisiting
2. Remove from `lib.rs` registration and delete the file
3. It's still in git history if anyone wants to recover it
4. Commit: `chore: remove <strategy> — no production admissions after N hours testing (see findings.json)`

**Current state (March 31):** 11 strategy files (6500+ lines) were added by the research
agent but none produced production admissions. These should be evaluated and cleaned up:
- Keep: tree2, tabu, polish, init (core, validated)
- Evaluate: crossover, sa (conceptually promising, need fair A/B test)
- Remove candidates: cayley, circulant, construct, gradient, lns, refine, relink, seidel, template
  (all marked "exhausted" without proper testing — preserve findings, remove code)

## Anti-patterns

- Don't implement multiple ideas in one cycle (this was violated — 11 strategies in one session)
- Don't refactor existing strategies (focus on new capabilities)
- Don't change scoring or server code (only worker/strategy code)
- Don't add ideas to the registry without implementing something first
- Don't skip the CI validation step
- Don't conclude tree2 is exhausted without verifying: (a) correct binary was used, (b) ILS
  restarts were enabled, (c) polish was deep enough (500+ steps), (d) ran for 2+ hours
- Don't add strategies faster than the experiment phase can test them
- Don't `git add` anything under `experiments/` — it's gitignored
- Don't leave dead code in the codebase — remove it and document in findings.json
