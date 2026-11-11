# Best order path review and exact evaluation reuse (2026-11-11)

The baseline is the working tree at task entry, including earlier result-cache
and vocal-policy work, not Git HEAD. Ordering version is 26. Existing unrelated
work is preserved. Timing inputs are immutable historical exports with stems
from `target/planner-perf-20261110/`, not a fresh audit of the current library.

## Current path and quality limits

1. Desktop preparation waits for stable analysis/stem evidence, then checks a
   complete-order cache. On a miss it loads directional cold pair scores.
2. At up to 40 tracks, the planner evaluates every directed pair. From 41 to
   200 it plans the 12 roughly closest successors of each track. Above 200 it
   uses rough tempo/key/energy costs without full pair planning.
3. Nearest-neighbour starts and directed relocation choose a cycle, opened
   after its weakest edge. This matrix operation is inexpensive compared with
   musical planning.
4. At up to 40 tracks, four bounded repair rounds simulate the actual carried
   entry, earliest exit, rate and pitch. The first edge is also revisited with
   the closing state. Safety failures, rushed passages and higher actual costs
   feed back into the pair matrix. Among evaluated rounds, fewer failed edges
   win first, then lower complete-cycle cost.
5. Each step compares current entrance alternatives, native-key and held-tempo
   alternatives, and real B-to-C forecasts. Each candidate fits parts and
   checks the rendered envelopes against vocal, melody, energy and foreground
   evidence. The shortlist preserves an early complete peak but is bounded.
6. Desktop publication then prepares the executable queue. An order result is
   only a permutation; repair currently retains step summaries, not complete
   plans for reuse by queue preparation. Live cue and carried-state differences
   must be checked before any future reuse can be safe.

This is a bounded heuristic, not a proof of a global optimum. A scalar cost
for A-to-B also cannot represent every possible B entrance and subsequent
C departure. More repair rounds alone increase repeated work without resolving
that representation limit. Large playlists additionally omit carried-state
repair. These are existing limitations, unchanged by this optimization.

## Retained implementation

- Cache the complete recording-level power reference alongside the immutable
  frame-power evidence. It used to traverse all bars and resample the same
  stem frames for every rendered candidate's foreground and energy checks.
  Nested scopes and workers share the exact value, including missing evidence.
  An unborrowed track computes directly. Identity is the immutable analysis
  address within its borrow, never a library ID or an across-request pointer.
- Stream the four source samples into band-power accumulation, eliminating a
  heap allocation for each query. Sample selection and addition order stay
  the same; gaps still use the original bar fallback.
- Sample gain, logarithmic filters and stem envelopes once per time position
  in continuity accounting, then reuse them across the three measured stems.
  The original products, sums and floating-point operation order are retained.

The lazy reference initializer only reads frames and performs a local sort;
it never launches or waits on nested Rayon jobs. No cache, locks, I/O or
allocation was added to an audio callback. Search budgets, score formulas,
safety gates, fallback policy and tie order are unchanged. ORDER_VERSION stays
26 because plan and ordering semantics are preserved.

## Further work indicated by the path

The next quality-oriented search change should retain several genuinely
distinct carried states during sequence search, compare complete-chain costs,
and retain a bounded best incumbent. Adjacency corrections should not erase
an alternative safe entrance merely because another context failed. Validate
this against an exhaustive small-set oracle and fixed full-plan comparisons
before changing persistent ordering semantics.

Before expanding the search, profile the remaining serial arrangement search
and reuse invariant source-window measurements. For queue handoff reuse, return
complete selected plans plus exact cues/options/evidence/carried-state keys;
only matching cold/live contexts may use them. Do not treat a cold pair score
or a display preview as an executable plan.

## Reproduction and validation

Artifacts live under ignored `target/order-path-20261111/`: task-entry and
optimized sources/executables, input hashes, phase timings, CPU time, sampling
captures and full excerpt plans. Runs are sequential; builds and tests follow
the comparison runs. Other applications remain active. macOS sampling itself
overlaps two short baseline windows and one optimized chain window. No audio was changed or played by this
measurement. Native UI latency, playback deadlines and listening quality are
not established by an offline planner benchmark.

Commands:

```sh
rtk cargo build --locked --release -p mixless-mixplan --features audit --example order-benchmark --example automix-audit
rtk proxy target/release/examples/order-benchmark ANALYSES.json --stems
rtk proxy env AUDIT_PLANS_OUT=plans.json target/release/examples/automix-audit ANALYSES.json --stems --json
```

## Matched observations

| Work | Task entry (s) | Optimized (s) |
| --- | ---: | ---: |
| 11-track Best order (planner only) | 443.21 | 386.72 |
| 110 directed pairs | 83.11 | 70.64 |
| Carried-state chain repair | 360.10 | 316.08 |
| Electronic, two transitions (process wall time) | 5.63 | 5.55 |
| MUG, two transitions (process wall time) | 32.61 | 30.97 |
| DnB, two transitions (process wall time) | 10.97 | 10.41 |

Full-order planner time fell 12.7%.
Whole-process CPU time fell from 2392.88 to
2045.02 seconds (14.5%).
The permutation remains `[6, 7, 0, 10, 3, 2, 8, 1, 4, 5, 9]`. All six excerpt plans are
byte-identical, including timing, strategy, offsets and control curves. These
are single matched observations on a loaded machine; the smaller excerpt
wall-time differences are not stable speedup guarantees. The full eleven-track
order comparison checks the permutation; only the six excerpt plans have a
complete serialized before/after comparison.

The optimized chain remains 81.7% of total planner time. This is a measured
reduction, not a resolution of the several-minute latency. The optimized
sampling capture still shows frame lookup, routed power, foreground checks
and arrangement evaluation as substantial remaining work.

## Integration results

- `cargo test --locked --workspace --exclude mixless-desktop`: 759 passed,
  28 ignored.
- `cargo test --locked -p mixless-mixplan --features trace level::evidence`:
  all three passed. Coverage includes exact reference values, worker/nested
  sharing, same-ID foreign tracks, edits after scope exit and cached absence.
- `./dev.sh --build`: passed and refreshed `target/app/Mixless.app` (dev,
  0.1.0-beta.5). The running app was not restarted.
- `git diff --check`: passed.

Timing runs completed before these checks started. Frozen before/after planner
source trees differ only in `level.rs`, `level/evidence.rs` and
`experience/signal.rs`; current integration also includes pre-existing working
tree changes. No listening or physical playback acceptance is claimed.
