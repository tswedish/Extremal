# Extremal — Technical Collaboration Prompt

You are helping me design and operationalize a project called **Extremal**.

Extremal is the new name for what I had been calling RamseyNet. The core is still distributed Ramsey graph search, but the identity is broader and more playful: mathematically interesting graphs are "gems," and one output of the system is not just leaderboard-worthy search results, but also visual artifacts such as deterministic pixel-art "Extremal Gems." I want the math, the systems engineering, the public-facing infrastructure, and the art angle to reinforce each other.

I want you to treat this as a serious technical project with room for fun and public participation.

What I want from you in this conversation:
1. Think deeply and concretely about how the experiment program should be run.
2. Make the case for a few distinct project tracks that are worth pursuing in parallel or in phases.
3. Propose tools and technologies that would make the project more powerful, including possible FPGA/digital logic co-processors, DSLs/compilers/languages, and automation scripts.
4. Design a human-in-the-loop operating model so I know when I should step in, what I should review, and what kinds of judgment calls are best left to me.
5. Ask me questions, and also suggest questions that I should be asking you.
6. Push back on weak ideas and identify what is probably overkill vs what is actually worth building now.

Important: do not give me a shallow brainstorm. I want a structured, technically serious response that ends with pointed questions for me.

---

## Project context

### Core problem

For given parameters R(k, ℓ), search for n-vertex graphs containing no k-clique and no ℓ-independent set. These are Ramsey graphs. A flagship target is R(5,5) on n=25 and nearby regimes (43 ≤ R(5,5) ≤ 48 is the known bound), but the infrastructure should generalize.

### Current architecture

**Server:**
- Axum REST API + SQLite ledger
- Accepts graph submissions, verifies them via exhaustive clique search
- Scores them with a 4-tier lexicographic system (clique counts → Goodman gap → automorphism group order → CID tiebreaker)
- Maintains leaderboards per (k, ℓ, n) triple with configurable capacity (currently 500)

**Worker:**
- Rust binary running search strategies
- Discovers candidate graphs, submits them to the server
- WebSocket dashboard for real-time monitoring
- Structured logging with `round_summary` lines for experiment analysis

**Current strategies:**
- `tree`: Beam search with single-edge flips. Systematic breadth-first exploration. Full clique recount per candidate (~53K subset checks for k=5, n=25). Finds many valid graphs but each evaluation is expensive.
- `tree2`: Incremental beam search. Same beam structure but with incremental violation delta computation (~1.7K subset checks per candidate instead of ~53K), flip-score-unflip (zero allocations in hot loop), cheap 64-bit fingerprint dedup instead of SHA-256. About 11x faster per round than tree.
- `evo`: Evolutionary SA with small population, persistent complement per individual, cross-round state persistence, leaderboard immigrant injection. Currently outperformed by both tree strategies for this problem.

**Verification:** Exhaustive backtracking clique search. For k=5, n=25, each full candidate evaluation examines up to C(25,5) = 53,130 potential 5-subsets per color (graph + complement).

**Graph representation:** Packed upper-triangular bitstring. For n=25: 300 bits = 38 bytes. SHA-256 of this encoding gives the content-addressed CID.

### Experiment infrastructure

We have a competitive iteration loop:
1. Run multiple workers/strategies against the same server/leaderboard
2. Collect structured logs (`round_summary` with strategy name, round number, iterations, elapsed_ms, discoveries, total_discoveries, total_admitted, total_submitted)
3. Compare key metrics: discoveries/min, admissions/min, admission rate, round time
4. Analyze what the winner does differently
5. Modify strategies, parameters, or mutation operators
6. Repeat

We have `./run experiment` which starts a server + two workers, logs everything, and prints a summary on Ctrl+C. We have `./scripts/analyze_experiment.sh` which produces a compact analysis from any experiment's logs.

---

## First experiment results (just completed)

**Head-to-head: tree vs tree2**, R(5,5), n=25, ~4.5 hours, both `--init leaderboard`, release mode.

```
Strategy: tree
  Rounds:             373
  Total discoveries:  1,521,991
  Total submitted:    1,951
  Total admitted:     1,051
  Admission rate:     53.9%
  Round time (ms):    avg=5023  min=4573  max=7110
  Admission trend:
    First 10% (37 rounds): 828 admissions
    Last 10% (37 rounds):  0 new admissions
    >> PLATEAU detected

Strategy: tree2
  Rounds:             2,109
  Total discoveries:  8,706,293
  Total submitted:    6,455
  Total admitted:     4,885
  Admission rate:     75.7%
  Round time (ms):    avg=331  min=113  max=1680
  Admission trend:
    First 10% (210 rounds): 2,329 admissions
    Last 10% (210 rounds):  51 new admissions
```

**Key findings:**
- tree2 is ~11x faster per round (331ms vs 5,023ms average)
- tree2 gets ~4.6x more leaderboard admissions
- Both find ~4K valid graphs per round, but tree2 runs 5.7x more rounds
- tree2 has higher admission rate (76% vs 54%), suggesting more diverse discoveries
- tree completely plateaued (0 admissions in last 37 rounds)
- tree2 is approaching plateau (51 admissions in last 210 rounds, vs 2,329 in first 210)
- The leaderboard (capacity 500) was heavily churned: ~6K total admissions means entries were replaced many times
- Round times for tree2 vary widely (113ms to 1,680ms) depending on seed quality from the leaderboard

**Implications:**
1. The incremental delta approach is a decisive win over full-recount beam search
2. The leaderboard is now nearly saturated — both strategies are plateauing
3. Next improvements must come from score optimization (Goodman gap, symmetry), not just finding more valid graphs
4. Diversity-aware search is needed to avoid rediscovering isomorphic/near-duplicate graphs
5. We need to think about what "better" means beyond violation count = 0

---

## What I want from you

### Step 1: Restate the project
Restate Extremal back to me as you understand it: mission, architecture, immediate goals, long-term vision, risks/tensions.

### Step 2: Ask me sharp questions
Ask me the most important questions needed to guide the next wave of development. Do not ask vague filler questions. Ask questions that affect architecture, experimentation, repo layout, operator workflow, public deployment, and creative direction.

Also: tell me what questions I should be asking myself, and what questions future contributors will ask.

### Step 3: Propose a phased plan
Give me a phased roadmap with explicit deliverables:
- next 2–3 days
- next 2 weeks
- next 1–2 months
- later / optional

For each phase: goals, key files/modules/scripts, success criteria, what to defer.

### Step 4: Argue for project directions
Make a serious case for each of these directions, including tradeoffs and whether to pursue now, later, or never:

1. **Algorithm-first path** — maximize better workers, improve mutation/evaluation/search quality
2. **Experiment-platform path** — build rigorous experiment harness, summaries, dashboards, journals
3. **Public-network path** — public server, public leaderboard, contributor workflows, cloud deployment
4. **Hardware/acceleration path** — GPU batching, FPGA co-processors for clique checking, SIMD kernels
5. **Language/DSL/compiler path** — strategy DSLs, worker configuration language, agent-generated strategies

### Step 5: Specific technical themes
Think concretely about:
- Incremental violation counting — what's the theoretical minimum work per candidate?
- Faster clique/independent-set detection — are there better algorithms than backtracking for this specific (k=5, n=25) regime?
- Batch candidate evaluation — can we evaluate many candidates in parallel (GPU, SIMD)?
- Graph fingerprints/dedup — is 64-bit XOR-fold good enough, or do we need better?
- Novelty search / archive design — how to maintain diversity in discovered graphs?
- Algebraic seeds and construction families — beyond Paley graphs, what's worth trying?
- Goodman-type objectives — how to optimize the Goodman gap during search, not just post-hoc?
- Automorphism-aware search — can symmetry information guide the search?
- Construction-based search — building graphs vertex-by-vertex instead of mutating complete graphs
- SAT/CSP approaches — are these relevant for R(5,5) n=25?
- Whether a strategy DSL is worth the complexity or premature
- How to structure agent-assisted (Claude/GPT) development workflows

### Step 6: Creative/art direction
Extremal should have a creative identity. I'm working on deterministic pixel-art renderings of graphs as "Extremal Gems." Think about:
- How to choose graphs worth visualizing
- How to derive visual style from graph invariants (symmetry group, spectrum, clique structure)
- How to make this deterministic and reproducible
- How to connect the search engine to a gem-rendering pipeline
- How this could help public engagement

### Step 7: Human-in-the-loop operating model
Design the operating model for me, the human operator:
- When should I intervene vs let things run?
- What decisions are best left to me?
- How should the system notify me?
- What's the right experiment review cadence?
- How can I efficiently "do things" for the coding agent when human judgment matters?

---

## My constraints and preferences

- **Hardware budget:** Savvy tech worker / weekend warrior. Desktop with gaming GPU. Interested in GPU/NPU/FPGA but only if practical.
- **Time budget:** Evenings and weekends. Need high-leverage tooling, not manual babysitting.
- **Technical depth:** I can do advanced things with clear guidance. I want to learn the math and share it.
- **Human-in-the-loop:** Keep me in the loop for now. I want to press execute, understand expected outputs, read summaries, and review analysis. Over time I want more autonomy, but not yet.
- **Leaderboard sizing:** Currently 500. I want to keep it there — the pressure is real (6K admissions into 500 slots). Would only grow it if admission rates collapse below ~1% for all strategies.

## My current hypotheses (challenge these if wrong)

1. The next experiment should NOT be another blind head-to-head. First: run tree2 with debug logging to get depth-level stats. Then build a tree2 variant with diversity-aware beam selection.
2. The most valuable outside thinking right now is: (a) better strategy design space, (b) sane phasing, (c) whether algebraic/construction-based ideas can produce better seeds or neighborhoods.
3. FPGA is probably premature, but I want a serious sanity check.
4. GPU batching for candidate evaluation is probably the highest-ROI hardware direction if software approaches plateau.
5. A strategy DSL would be cool but is premature until we've exhausted manual Rust strategy iteration.

---

Please begin with your restatement, then questions, then the phased plan. Be opinionated. Be concrete. Push back on weak ideas. Teach me math as we go.
