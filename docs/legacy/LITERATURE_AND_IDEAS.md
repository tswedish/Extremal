# Literature Review and Strategy Ideas for Extremal

Research notes from key Ramsey number papers, with concrete ideas for
improving Extremal's search strategies.

## Papers Reviewed

### 1. Radziszowski, "Small Ramsey Numbers" (Dynamic Survey, updated Sep 2024)

The definitive reference. Key facts for R(5,5):
- **43 ≤ R(5,5) ≤ 48** — the gap has been open since 1997 (lower) / 2017 (upper)
- Widely conjectured that R(5,5) = 43
- There are exactly **656 known R(5,5) graphs on 42 vertices** (McKay & Radziszowski 1997)
- Any new R(5,5)-42 graph would need to be very structurally different from all 656 known ones
  (no shared 37-vertex subgraph with any of them)

**Implication for Extremal:** Our search at n=25 is well within the valid range (25 < 43),
so valid R(5,5) graphs on 25 vertices definitely exist. The question is finding *diverse*
and *high-quality* ones, not whether they exist.

### 2. Exoo & Tatarevic, "New Lower Bounds for 28 Classical Ramsey Numbers" (2015)

The most methodologically relevant paper for Extremal. They improved 28 Ramsey bounds
using a pipeline of heuristic search methods.

#### Key Algorithms

**Steepest Descent with Tabu Search (Algorithm 1):**
- Considers ALL possible edge recolorings, picks the one that minimizes score
- Maintains a FIFO tabu list (L=1000) of recent moves to prevent cycling
- This is different from our beam search: they're greedy (take the single best move)
  with anti-cycling, while we explore a beam of multiple candidates

**Simulated Annealing with Focused Edge Set (Algorithm 2):**
- Key innovation: **only flip edges that participate in bad subgraphs** (monochromatic
  cliques/independent sets). This focuses computation where it matters.
- The "guilty edge" set I is recomputed every |V|/4 iterations
- Temperature and cooling rate are empirical, depend on graph size and structure

**Adaptive Weighted Scoring:**
- NOT just raw violation count. Uses `score = Σ w_c × f_c` with adaptive weights
- **Weight ratio rule for R(s,t) with s < t:** `t/s ≤ w_s/w_t ≤ (t/s)²`
  For R(5,5) this is trivial (s=t, so equal weights), but matters for R(5,k) with k≠5
- **Dynamic weight update:** `w_c = (K × w_c + f_c) / ((K+1) × Σf_i)`, K in [10,100]
  Prevents oscillation between over-eliminating cliques vs independent sets

#### Search Space Structures

Three levels of decreasing symmetry constraint:

1. **Circulant colorings:** Adjacency matrix is a symmetric circulant. Search space: n/2 bits.
   Fastest to search, often finds good initial solutions.
2. **Block-circulant colorings:** Matrix partitioned into m×m array of d×d circulant blocks.
   Intermediate search space. Good for refining circulant solutions.
3. **Single-edge recoloring:** Full C(n,2) search space. Slowest but most flexible.

**Pipeline:** circulant → block-circulant → single-edge refinement.

#### Key Finding

**Circulant colorings consistently outperform general Cayley colorings.** The authors
attribute this to cyclic groups having elements of larger average order. This suggests
that searching over circulant parameter spaces may be more efficient than random search
in the full graph space.

### 3. Codish et al, "Computing R(4,3,3) = 30" (2015)

Proved R(4,3,3) = 30 using SAT with abstraction and symmetry breaking.

#### Methodology

- Encode Ramsey graph constraints as a SAT formula
- **Abstraction:** First enumerate all valid colorings on a smaller graph, then extend
- **Symmetry breaking:** Use canonical forms to prune isomorphic search branches
- Computed the full set R(3,3,3;13) = 78,892 Ramsey colorings as a prerequisite

**Implication for Extremal:** SAT-based approaches work well for exact enumeration
near the Ramsey boundary. For our use case (finding diverse graphs at n=25, well below
the boundary), SAT could find solutions in unexplored regions but wouldn't be competitive
with local search for throughput.

### 4. Angeltveit & McKay, "R(5,5) ≤ 48" (2017)

Proved the current best upper bound by checking ~2 trillion cases.

#### Key Technical Insights

**Degree structure of R(5,5) graphs:**
- In any graph in R(5,5,48), every vertex has degree 23 or 24
- This extreme degree regularity is a general property near the Ramsey boundary

**R(4,5) catalogue:**
- Exactly **352,366 graphs** in R(4,5,24) — our target regime!
- Edge counts range from 264-368 (density 0.46-0.64)
- Most graphs have ~123 edges (density ~0.50)
- Minimum degree 6-11, maximum degree 10-13
- This means R(5,5) graphs on n=25 are a subset of R(4,5,24) graphs

**Constraint propagation for Ramsey:**
- "Interval collapsing" with 11 rules based on potential cliques/independent sets
- Unit propagation: when a clause has one unknown variable, force it
- This achieves hundreds of thousands of evaluations per second per core

**Structural rigidity near the boundary:**
- For R(5,5) graphs on 38 vertices: 647,424 graphs found, but all came from a
  SINGLE overlap structure. Extreme structural rigidity.
- For 37 vertices: 15,244 graphs from only TWO overlap structures.
- For 34-36 vertices: NO valid gluings at all.

**Implication for Extremal:** The R(4,5,24) catalogue tells us what the "universe"
of valid n=25 graphs looks like: ~50% density, fairly regular degree sequences,
degree range [6,13]. Our search should be biased toward this structural profile.

---

## Concrete Ideas for Extremal

### HIGH PRIORITY — Implement Now

#### 1. Focused Edge Flipping (from Exoo-Tatarevic Algorithm 2)

**Current state:** tree2 and evo flip random edges from the full edge list (300 edges
for n=25).

**Improvement:** Only flip edges that participate in violations. When violations > 0,
identify the "guilty" edges (edges contained in monochromatic 5-cliques or 5-independent
sets). Only mutate those edges. Recompute the guilty set periodically.

**Expected impact:** Dramatic reduction in wasted mutations. Most of the 300 edges are
irrelevant to the current violations — flipping them can't help. Focusing on the ~20-50
guilty edges means each mutation is ~6-15x more likely to reduce violations.

**Implementation:** After computing violations, iterate over the cliques/independent sets
found and collect the edges. Store as a `HashSet<(u32,u32)>`. In the inner loop, sample
from this set instead of `all_edges`. Recompute every 100-500 iterations.

**Effort:** Low (1 evening). Can be added as a config option to tree2 or as a new strategy.

#### 2. Circulant Graph Search (from Exoo-Tatarevic)

**Current state:** We use Paley graphs as seeds but don't exploit circulant structure
during search.

**Improvement:** Implement a circulant search mode. A circulant graph on n=25 vertices
is defined by a connection set S ⊆ {1,...,12} (since S must be symmetric: d ∈ S ⟹ 25-d ∈ S).
That's only 2^12 = 4,096 possible connection sets. Exhaustively enumerate all valid
R(5,5) circulant graphs on 25 vertices.

**Expected impact:** Finds all circulant solutions instantly (seconds). These are
algebraically structured graphs with high automorphism groups, which score well on
our T3 (symmetry) tier. They also serve as high-quality seeds for local search.

**Implementation:** A standalone function that enumerates 2^12 connection sets, builds
each circulant graph, checks for 5-cliques and 5-independent sets. Output all valid ones.

**Effort:** Very low (a few hours). Could be a test or a script.

#### 3. Tabu List for tree2 (from Exoo-Tatarevic Algorithm 1)

**Current state:** tree2 uses a fingerprint-based `seen` set for dedup, but this resets
each round. There's no mechanism to prevent revisiting the same graph regions across rounds.

**Improvement:** Maintain a tabu list of recently-explored graph fingerprints across rounds
(via `carry_state`). When scoring candidates, penalize or skip those in the tabu list.

**Expected impact:** Forces exploration into new regions instead of re-converging on the
same local optima from different seeds. Particularly valuable when the leaderboard is
saturated and all seeds lead to the same basins.

**Effort:** Low (1 evening). Add a `HashSet<u64>` to `carry_state`, insert fingerprints
of submitted graphs, check during candidate scoring.

### MEDIUM PRIORITY — Next Wave

#### 4. Block-Circulant Search

Search over block-circulant structures (n=25 = 5×5 or 25×1). This is the
intermediate search space from Exoo-Tatarevic — larger than pure circulant but
much smaller than full edge space. Block-circulant graphs often have good
structure for Ramsey problems.

#### 5. Score-Aware Valid-Space Walk

Once tree2 finds a valid graph, walk through the valid-graph space by flipping
edges that maintain validity. Optimize for Goodman gap and symmetry. This is
where the Angeltveit-McKay insights about degree regularity help: prefer
mutations that move toward more regular degree sequences.

#### 6. Degree-Biased Mutations

The R(4,5,24) catalogue shows valid graphs have min degree 6-11 and max degree
10-13. Bias mutations toward moves that regularize the degree sequence:
- If a vertex has high degree, prefer removing its edges
- If a vertex has low degree, prefer adding its edges
- Target degree range [10,12] based on the catalogue statistics

#### 7. SAT Solver Integration

Encode R(5,5) constraints for n=25 as SAT: 300 variables, ~106K clauses.
Use a modern solver (CaDiCaL via C FFI or varisat in pure Rust) to find
solutions. After finding one, add a blocking clause and find another.
This accesses regions that local search may never reach.

### LOW PRIORITY — Future / Research

#### 8. Constraint Propagation Engine

Implement Angeltveit-McKay style interval collapsing for R(5,5). Given a
partial graph (some edges decided, others undecided), propagate constraints
to force or eliminate edges. This could accelerate tree2 by pruning the
candidate space: before trying all 300 edge flips, propagate constraints
to identify edges that are forced.

#### 9. Catalogue Mining

The 352,366 graphs in R(4,5,24) are publicly available (from McKay's website).
Download them, convert to RGXF, and submit to Extremal. This would instantly
populate the leaderboard with the complete known catalogue. Then local search
operates on the edges of the known universe rather than rediscovering it.

#### 10. Multi-Color Extension

The splitting method from Exoo-Tatarevic: take a valid R(5,5) graph on n
vertices, split the K5-free subgraph into two K3-free subgraphs to get an
R(3,3,5) coloring. This extends Extremal's scope to multicolor Ramsey problems.

---

## Strategy Competition Roadmap

Based on the literature, here's the recommended sequence of strategy experiments:

| Experiment | What | Expected Result |
|-----------|------|-----------------|
| **E1** | tree2 + focused edge flipping | Faster convergence (fewer wasted mutations) |
| **E2** | Circulant enumeration | Catalogue of all circulant R(5,5)-25 graphs |
| **E3** | tree2 + cross-round tabu | Better diversity, less re-convergence |
| **E4** | tree2 + degree-biased mutations | Better graph quality (more regular degrees) |
| **E5** | SAT solver for diverse seeds | Access to new graph regions |
| **E6** | Score-aware valid-space walk | Better Goodman gap and symmetry scores |
| **E7** | Full pipeline: SAT → circulant → tree2 → score walk | Combined approach |

Each experiment should run overnight (8+ hours) with the fleet sweep infrastructure
and be compared against the current tree2 baseline using admits/hr and score quality.

---

## References

1. Radziszowski, S. "Small Ramsey Numbers." Electronic Journal of Combinatorics,
   Dynamic Survey DS1, updated Sep 2024. https://doi.org/10.37236/21

2. Exoo, G. and Tatarevic, M. "New Lower Bounds for 28 Classical Ramsey Numbers."
   Electronic Journal of Combinatorics 22(3) (2015) #P3.11. arXiv:1504.02403

3. Codish, M., Frank, M., Itzhakov, A., and Miller, A. "Computing the Ramsey
   Number R(4,3,3) Using Abstraction and Symmetry Breaking." arXiv:1510.08266

4. Angeltveit, V. and McKay, B.D. "R(5,5) ≤ 48." arXiv:1703.08768

5. McKay, B.D. and Radziszowski, S. "R(4,5) = 25." Journal of Graph Theory
   19(3):309-322, 1995.
