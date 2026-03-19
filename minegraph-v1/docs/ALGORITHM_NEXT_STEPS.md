# Algorithm Improvement Ideas

Prioritized list of algorithmic improvements for future experiments.
Based on the RamseyNet prototype's research and experiment results.

## Priority 1: Circulant Enumeration

Exhaustively enumerate all 2^(floor(n/2)) circulant graphs on n vertices.
For n=25, that's 4096 graphs — takes seconds on one core.

Circulant graphs have the highest automorphism group orders (|Aut| = n * k for
some k), which gives them a T3 scoring advantage. The best seeds for tree2
search are circulant graphs with zero violations.

**Implementation**: Add a `circulant` init mode that generates and scores all
circulant graphs, submits the best ones, and uses them as seeds.

## Priority 2: Cross-Round Tabu

Maintain a set of explored graph fingerprints across rounds via `carry_state`.
Currently each round starts fresh with no memory of what was already explored.

**Implementation**: Return a `HashSet<u64>` (fingerprints) as `carry_state` from
`SearchResult`. The next round's `SearchJob` passes it back. tree2 checks the
tabu set before adding candidates to the beam.

**Expected impact**: Prevents re-exploring the same local optima across rounds.
The prototype measured ~15% improvement in discovery rate.

## Priority 3: Score-Aware Valid-Space Walk

Once a valid graph is found (violations = 0), walk within the valid-graph space
by only making flips that keep violations at zero, while optimizing for:
- Lower Goodman gap
- Higher |Aut(G)| (more symmetry)

**Implementation**: New search phase after tree2 finds a valid graph. For each
edge, check if flipping it maintains validity. Among valid flips, prefer those
that improve the score.

## Priority 4: Population-Based Search (Evo Strategy Port)

Port the evolutionary simulated annealing strategy from the prototype.
Maintains a population of candidate graphs with crossover and mutation.

**Key features to port:**
- Population of N candidates with tournament selection
- Crossover: combine edge sets from two parents
- Simulated annealing temperature schedule
- Immigrant injection from leaderboard

## Priority 5: Multi-Strategy Fleet

Run different strategies simultaneously and let them compete. The fleet script
already supports multiple workers — each can run a different strategy.

**Implementation**:
- Add `--strategy evo` to worker CLI
- Fleet script assigns strategies round-robin: tree2/tree2/evo/evo
- Compare admits/hr across strategies in experiment analysis

## Priority 6: Adaptive Hyperparameters

Automatically tune beam_width, max_depth, sample_bias based on admission rate.
If the leaderboard is saturated (no admissions for N rounds), increase noise
or switch to exploration mode.

**Implementation**: Engine watches admits/round and adjusts strategy_config
dynamically. Could also adjust noise_flips.

## Priority 7: Graph Database Seeding

Use known Ramsey graphs from the literature as seeds instead of Paley graphs.
The Exoo-Tatarevic paper catalogs known R(5,5) constructions.

**Implementation**: Add a `--seed-file` flag that loads graph6 strings from
a file and uses them as init graphs.

## Priority 8: Parallel Beam Search

Split the beam across multiple threads within a single worker. Currently tree2
is single-threaded.

**Implementation**: Use rayon to parallelize the inner loop over beam parents.
Each thread scores candidates independently, then merge results.

**Caution**: The bitwise neighbor masks are already very cache-friendly.
Parallelizing may not help much for small n (25 vertices) due to overhead.

## Experiment Log

| ID | Description | Result |
|----|-------------|--------|
| E001 (prototype) | Baseline tree2 vs tree1 | tree2 11x faster |
| E002 (prototype) | Focused edge flipping | Neutral for beam search |
| E003 (prototype) | Overnight 16-worker fleet | 539 admits/hr baseline |
| E004 (prototype) | Hyperparameter sweep | beam_width=80, max_depth=12, bias=0.8 wins |
| E005 (v1) | 10-min capacity test | 162 admitted, capacity 500->2000 headroom |
| E006 (v1) | 8h overnight, 8 workers | 6012 admitted, saturated at 2000 |
