# Deterministic planner parallel search

Date: 2026-11-11. Baseline: `8ac5dbfafb2a20acd7d17e15ec8b5c98a86ccf3d`.
This follows the [planner repair review](2026-11-11-planner-repair-and-review.md).
The user authorized the source-evidence, arrangement-seed and single-result
search optimizations, with unchanged planner judgments.

## Implementation and review

- Vocal-pattern and continuation evidence is shared across the immutable request
  and its workers. Each scope keeps a small local map in front of 32 shared
  shards. Each evidence kind has at most 131,072 shared entries; eviction only
  recomputes evidence. Keys use the actual borrowed analysis identity and the
  exact frame index or `f32` time bits. Failures are cached too. Neither cue
  rounding nor track-ID equivalence is used. Independent requests, edited
  inputs and unborrowed tracks cannot inherit a previous classification.
- Arrangement seeds run as four tonal jobs, each retaining all five kit anchors,
  plus the existing alternative-arrangement job. Each seed keeps its original
  serial coordinate descent and melodic fitting. Results are flattened in the
  original order and deduplicated by exact shape bits before rendering. The
  shared shape-cost cache coalesces identical cold work under a shard lock.
  This is safe specifically because that cost function is a leaf calculation:
  it performs no other locking, evidence queries or nested worker scheduling.
- Single-result searches evaluate ordered batches of exit/entry anchors. Within
  an anchor, mode and length order are unchanged. A batch uses the resolved
  prefix's score bound. If an earlier result changes that bound, later anchors
  are replayed with the new bound, including earlier failures. This matters
  because pruning a direct plan can open its prepared fallback; simply reducing
  speculative winners by final merit would not establish equivalence. Startup
  stays serial until an incumbent exists, preserving the initial -1 sentinel.
- All work uses the existing bounded Rayon pool and QoS propagation. Thread-local
  scope guards stay on their entering thread and restore nested state. General
  source-cache misses release locks before measuring; the complete-plan memo
  still never waits for an in-flight nested search. Trace mode retains serial
  search and uncached complete plans so rejection collectors remain observable.

No candidate budget, duration set, voice/foreground gate, score, tie rule,
manual-cue authority or chain-repair round was removed. Carried-state transitions
remain sequential. Analysis extraction and playback callbacks are unchanged.
Analysis and order versions remain **27** because this is an exact-result
optimization, not a changed musical policy or an expansion of the search space.

## Matched cold Best-order measurement

Immutable release executables used the same version-27 eleven-track export,
with stems enabled and fresh in-process caches. All three runs wrote the same
order audit. Hardware: Apple M5 Max, 18 logical CPUs, macOS 27.2 (26B123),
Rust 1.99.1. No own builds, tests, audio audits or sampling profiles overlapped
these timings. Unrelated user applications and an existing training process
remained active. These are single offline observations, not statistical latency
bounds or live handoff measurements.

| Implementation | Directed pairs | Carried-state repair | Total |
| --- | ---: | ---: | ---: |
| Frozen baseline | 68.630 s | 343.940 s | 412.572 s |
| Shared evidence and 21 seed jobs | 70.886 s | 293.663 s | 364.551 s |
| Final five seed jobs, sharded costs and ordered single search | 58.073 s | 178.372 s | 236.446 s |

Final elapsed time decreased **42.7%** (1.745 times throughput for this request),
and repair time decreased **48.1%**. The intermediate 21-job layout spent
628.10 CPU-seconds in the kernel; coarser seed jobs and sharded costs reduced
that to 101.66. Final total process CPU time increased from 1,916.09 to 2,146.10
seconds (12.0%), including speculative work and required replay. Peak RSS grew
from 400.33 to 491.91 MiB, a 91.58 MiB tradeoff. Thus the wall-time gain does
not imply an equal reduction in CPU work or memory.

Both optimized runs produced all **160 byte-identical audit files**: 110 directed
pairs, 45 distinct carried-state steps, all four repair-round records and the
matrix. The selected order remains `[6,7,0,10,3,2,8,1,4,5,9]`. This comparison
includes full generated plans, source clocks, automation, scores and corrections,
not just the playlist permutation.

## Validation

The offline artifacts are in ignored `target/planner-parallel-20261111/`:
frozen executables, input/source/binary hashes, `/usr/bin/time -l` records,
complete order audits, and serialized fixed-queue comparisons. Source hashes
were checked against the measured final executable's provenance after review.

- Planner tests: 279 passed, 2 ignored. Trace mode: 274 passed, 8 ignored.
  Added checks cover worker/request sharing, exact adjacent times, same-ID
  foreign analyses, nested restoration, edits after request completion, bounded
  eviction and negative evidence, every ordered seed shape/cost, full serial vs
  batch plan comparison, manual cues, carried rates, missing grid, key conflict,
  tempo differences, failures and ties.
- Required workspace tests: 775 passed, 30 ignored. `./dev.sh --build` produced
  the development bundle without launching it. Formatting and diff checks pass.
  Planner Clippy completed with existing style warnings; no new warning points
  into the added implementation.
- Four fixed queues produced 84 full plans, all byte-identical to the frozen
  version-27 plans from the preceding review. Their cycle inputs and baseline
  files were hashed before this comparison. An additional 12 plans without
  stems were freshly generated by both immutable executables and matched byte
  for byte. Fixed-queue validation overlapped the workspace/build checks, so its
  durations are not presented as isolated performance measurements.

These checks preserve the existing planner's decisions on the tested inputs.
They do not prove an unbounded global optimum, musical-analysis ground truth,
new subjective listening acceptance or live audio deadline performance. Existing
vocal-risk and terminal-handoff findings in the previous audit remain open.
The running app, live library, manual cues and source audio were not modified.
