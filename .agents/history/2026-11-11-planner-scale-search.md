# Planner computation reuse on larger playlists

Date: 2026-11-11. Baseline: `2f494afa9a9fa0e40d00e7b74c1f9801a1e41905`.
This follows the [parallel-search optimization](2026-11-11-planner-parallel-search.md).
The user requested further speedups and larger-list measurements, accepting
additional CPU and memory while preserving planner decisions.

## Implementation and review

- Routed power checks share source-clock positions, stem selections, channel
  gains, EQ and filter factors within one immutable rendered candidate. Full
  and foreground powers are cached separately at exact `f32` time bits. The
  key includes the actual plan and both borrowed analysis identities. The
  thread-local scope restores nested state, cannot move between threads, and
  ends before the plan can be edited. Foreign plans or analyses use the normal
  computation. Its 8,192-sample bound only evicts computed values; it does not
  reduce sampling or candidate coverage. No cache borrow spans computation.
- Arrangement scoring prepares source-only band totals, sounding-part tests,
  foreground weights and frame intervals once for all shapes. It retains the
  original floating-point accumulation order, including the flat two-deck
  total. Movement scoring calculates the melodic envelope it uses, instead of
  calculating and discarding all other envelopes. Filter cutoffs are reused
  between power estimation and rendering. All safety gates and score terms
  remain in their original order.
- The request memo grows with input size instead of evicting after 256 plans:
  `clamp(n * (n + 64), 256, 8192)` entries and
  `clamp(n * 32, 64, 512)` MiB, with saturating integer arithmetic. Allocation
  remains lazy and the byte budget is an estimated stored-plan budget, not a
  process RSS cap. Exact cues, carried state, options, failures and complete
  candidate vectors retain their existing keys and semantics. Oversized values
  bypass storage. Workers still never wait for in-flight plan computations.

This is computation reuse on the existing bounded worker pool, not additional
candidate pruning. Candidate order, tie rules, search depth, cue authority and
musical safety thresholds are unchanged. Analysis extraction and audio callbacks
are unchanged. Analysis and order versions remain **27**.

Frozen original route and cost functions are retained under `cfg(test)` for
bitwise comparisons. Checks cover filters, delayed incoming audio, missing
stems, source gaps, empty clocks, NaNs, adjacent representable times, nested
scopes, foreign identities and edits after a scope ends. Cost comparisons cover
160 shapes across all three tonal relationships, including both acceptance and
rejection. Memo tests exercise eviction, oversized bypass and bounded growth.

## Matched cold measurements

Inputs come from immutable version-27 analysis exports of four existing
regression playlists, containing 76 distinct tracks. The 29-track input is one
complete playlist; the 64-track input interleaves the four playlists without
duplicating track IDs or altering analyses. These are regression materials,
not a new independently annotated or held-out audio set.

Both versions use frozen release executables, stems, fresh process caches and
the same complete order audit. Hardware: Apple M5 Max, 18 logical CPUs,
macOS 27.2 (26B123), Rust 1.99.1. No own builds or tests overlap these timings;
unrelated user applications and a pre-existing training process remain active.
Each 29-track run includes two two-second sampling profiles, one during pair
evaluation and one during chain repair. The 64-track runs have no sampling.
These are single offline observations, not latency distributions.

| Tracks and unchanged search policy | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| 29: all 812 directed pairs and four chain-repair rounds | 476.385 s | 408.542 s | 14.2% |
| 64: 768 directed pairs, no chain repair | 517.167 s | 433.936 s | 16.1% |

For 29 tracks, pair evaluation falls from 226.726 to 193.563 seconds and chain
repair from 249.646 to 214.968 seconds. Total process CPU time falls from
5,292.31 to 4,552.81 seconds (14.0%). Maximum RSS rises from 693.67 to
730.59 MiB. The request memo retains 1,393 entries, peaks at 232.15 MiB of
estimated stored plans and records zero evictions, compared with 1,156 baseline
evictions. It records 82 hits instead of 63. Cache growth is therefore only
part of the speedup; per-candidate arithmetic reuse matters too.

All **931** 29-track audit files are byte-identical: 812 pairs, 114 distinct
carried-state steps, four repair records and the cost matrix. This compares
full plans, source clocks, controls, costs and corrections, not only the final
permutation.

For 64 tracks, all **769** audit files (768 pairs and the matrix) are also
byte-identical, as is the final order. Process CPU time falls from 7,424.37 to
6,397.85 seconds (13.8%); maximum RSS rises from 580.64 to 685.03 MiB. The memo
stores 768 plans, peaks at 157.23 MiB and records zero hits in both versions:
this cold-pair gain comes from arithmetic reuse, not the larger plan memo.

The existing policy changes above 40 tracks: only the 12 rough-nearest
successors per track receive full pair planning, and carried-state order repair
is skipped. Above 200 tracks the current policy uses rough costs only. This
change does not modify those limits. Do not interpret comparisons across those
boundaries as the scaling of an exhaustive search, or claim a global optimum.
Musically plausible pairs can require more arrangement fitting than pairs
rejected early, so pair count alone does not predict elapsed time.

## Validation

- Planner unit tests: 283 passed, 2 ignored. Trace mode: 278 passed, 8 ignored.
  Release tests pass for both the original/new scoring comparison and the two
  routed-power/scope comparisons.
- Required workspace tests: 779 passed, 30 ignored. `./dev.sh --build` succeeds
  and refreshes the development bundle without launching it. Formatting and
  diff checks pass. Planner Clippy completes with style warnings, including
  the indexed shape loop and the negated partial-order comparison used to
  preserve the original NaN incoming-start behavior; it is not warning-free.
- The selected 64-track cycle plus its first three tracks produces **66**
  carried-state plans, all byte-identical between the frozen executables.
  Both versions retain the same three existing fallback transitions, and
  neither reports a plan failure. The original 11-track cycle adds **13**
  byte-identical plans; a five-track queue without stems adds **4** more.
  These comparisons overlap the workspace/build checks, so their durations
  are not treated as isolated performance measurements.
- No implementation changes were made after freezing the measured binaries.
  The source hashes were checked against their provenance after review.

## Reproduction and evidence limits

Ignored artifacts live in `target/planner-scale-20261111/`: immutable inputs,
before/after release binaries, source and binary hashes, time/RSS records,
profiles, memo statistics, complete order audits and comparison results.
`bench.py` runs the comparisons sequentially; `validate.py` compares complete
plans along the selected 64-track cycle with carried state, the original
11-track cycle, and a no-stems queue. Audit inputs and generated plans contain
personal-library data and are not committed.

The measurement entry point is:

```sh
cargo run --locked --release -p mixless-mixplan --features audit \
  --example order-benchmark -- ANALYSES.json --stems
```

Set `MIXLESS_ORDER_AUDIT` to a fresh directory to compare all order decisions.
For continuous planning use the `automix-audit` example with `--stems --json`
and `AUDIT_PLANS_OUT` pointing to an output JSON file. These runs simulate
planning; they do not play audio or establish subjective listening quality.
Unchanged plans establish regression parity on these inputs, not analysis
ground truth, an unbounded optimum or live handoff deadline performance.
