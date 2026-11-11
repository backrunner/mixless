# Production planner acceleration

Date: 2026-11-12. Baseline: `5f2180f24653c75fd58957111f71a9fbf6ccb543`.
The user requested a measured production speedup after the
[Metal comparison](2026-11-12-planner-gpu-shadow.md).

## Mechanism and decision preservation

A fresh two-second sample of the frozen baseline's eleven-track replay found
substantial time in arrangement gain evaluation, cost lookup and hashing.
Metal remained unsuitable as an authoritative selector: the prior full
comparison had found five different width winners. This change accelerates
the normal CPU planner without depending on a GPU score tolerance.

The scorer now distinguishes two kinds of failed calculation. Vocal overlap,
tonal overlap duration and missing foreground depend on the exact vocal,
melodic and bass fades, together with the immutable source frames. Drum fades
cannot repair any of these failures. A bounded cache therefore shares those
failed gates across drum variants, keyed by the exact bit patterns of all
twelve endpoints of the other six fades. Successful costs and level-loss
rejections retain the complete sixteen-value shape key because routed power includes
drums. A level failure encountered before a shared gate remains shape-specific.
Neither a promising score nor a numerical tolerance authorizes this reuse.

Each immutable arrangement search owns 32 shards, each with at most 128 full
shape results and 128 shared rejection proofs. Entries allocate lazily;
eviction only repeats calculations. Shapes with the same shared inputs use
the same shard, and the existing leaf scorer runs under that shard's lock.
There are no nested jobs, evidence lookups or additional locks inside scoring.
GPU observations still occur after releasing the cache lock.

On AArch64 targets with NEON enabled, arrangement scoring evaluates each deck's
four fade lanes using SIMD. It retains the original operation order and uses
division rather than a reciprocal approximation. Comparison/select operations
retain the scalar clamp's signed-zero and NaN behavior. Degenerate/reversed
fades retain their original step behavior. No cross-lane sum is vectorized,
and power/logarithm functions retain their existing CPU implementations. Other
targets use the scalar envelope calculation. Playback/rendering envelopes and
the audio callback are unchanged.

The implementation uses Rust's documented
[vector division](https://doc.rust-lang.org/core/arch/aarch64/fn.vdivq_f32.html)
and [bitwise selection](https://doc.rust-lang.org/core/arch/aarch64/fn.vbslq_f32.html)
intrinsics. Unsafe operations are limited to loading/storing four initialized
local floats with compile-time NEON gating; no structure layout casts are used.

Candidate enumeration, scoring thresholds, tie order, manual cue authority,
search depth and chain-repair policy remain unchanged. `ORDER_VERSION` stays
28 and `ANALYSIS_VERSION` stays 27. The normal desktop planner enables these
CPU changes; Metal remains optional offline comparison only.

## Measurements

Hardware: Apple M5 Max, 18 logical CPUs, macOS 27.2 (26B123), Homebrew Rust
1.99.1. Timed executables are frozen release builds with production parallelism
and stems. Each invocation has fresh process caches and no persistent pair
cache. This task runs no builds, tests, profiles or GPU audits concurrently
with the timings. Unrelated user processes are left untouched.

Initial eleven-track carried-state replay, producing thirteen identical plans:

| Implementation | Wall seconds | User + system CPU seconds |
| --- | ---: | ---: |
| Baseline | 37.384 | 249.82 |
| Shared rejection proofs | 34.459 | 221.67 |
| Shared proofs and SIMD | 33.189 | 215.56 |

These are single preliminary observations.

The final 29-track cold Best-order comparison exercises all 812 directed pairs
and four repair rounds. It preserves the final order and all 931 audit files
byte for byte: the 812 pair plans, 114 distinct carried-state steps, four repair
records and the cost matrix.

| 29-track phase | Baseline seconds | Optimized seconds | Reduction |
| --- | ---: | ---: | ---: |
| Pair planning | 247.848 | 175.456 | 29.2% |
| Carried-state repair | 221.605 | 195.418 | 11.8% |
| Complete Best order | 469.465 | 370.886 | 21.0% |

Total process CPU time decreases from 4,698.56 to 4,170.78 seconds (11.2%).
Maximum RSS decreases from 794,230,784 to 749,027,328 bytes (5.7%). The larger
wall-time reduction may also reflect scheduling and contention; it does not
imply an equal reduction in total CPU work. This is one adjacent full-search comparison,
not a latency distribution or a guaranteed percentage on other inputs/devices.

The 64-track comparison preserves all 768 pair plans, the cost matrix and the
final order: all 769 audit files match byte for byte. The existing twelve-
successor policy applies, with no carried-state order repair. This size runs
the optimized executable first and the baseline second, reversing the order
of the 29-track comparison.

| 64-track phase | Baseline seconds | Optimized seconds | Reduction |
| --- | ---: | ---: | ---: |
| Pair planning | 565.751 | 479.062 | 15.3% |
| Complete Best order | 565.839 | 479.125 | 15.3% |

Total process CPU time decreases from 6,615.34 to 5,966.93 seconds (9.8%).
Maximum RSS increases from 623,362,048 to 628,768,768 bytes (0.9%). This is also
one adjacent comparison, with unrelated user workload left active. Historical
64-track timings from the earlier day are not used as the control.

The final eleven-track carried-state replay then runs in baseline/optimized/
optimized/baseline order, with fresh processes and the same thirteen full plans
in every run:

| Run | Wall seconds | User + system CPU seconds |
| --- | ---: | ---: |
| Baseline 1 | 40.735 | 255.86 |
| Optimized 1 | 40.497 | 219.71 |
| Optimized 2 | 33.130 | 216.58 |
| Baseline 2 | 38.267 | 251.43 |

Mean wall time falls from 39.501 to 36.813 seconds (6.8%), while mean CPU time
falls from 253.645 to 218.145 seconds (14.0%). The overlapping wall-time ranges
show variability despite the consistent CPU saving. Two observations per
version do not establish a latency distribution or a per-transition deadline.
These are whole carried-state queue replays, not full Best-order searches.

The four regression queues, 64-track cycle and no-stems regression preserve
all 154 complete output plans byte for byte. The 64-track cycle contributes
66 carried-state plans and takes 428.545 seconds in this run. Because this was
longer than the historical comparison run, an additional fresh baseline control
was measured after the final GPU diagnostic, with no overlapping task workload.
It takes 478.376 seconds and again produces the same 66 plans byte for byte.
The optimized cycle is 10.4% faster in this single comparison. CPU time falls
from 2,589.57 to 2,348.17 seconds (9.3%), and maximum RSS falls from 654,245,888
to 550,207,488 bytes (15.9%). This confirms a gain against the fresh control;
the older timing is not used to claim either a regression or an improvement.

The final complete GPU diagnostic compares all 50,839,830 observed queries in
the eleven-track cycle with no drops or uncompared queries. Its acceptance,
batch-selection, strict-comparison and score-bit difference counts remain
0, 0, 8 and 431,410 respectively, matching the prior baseline report. All
thirteen full plans remain unchanged. Actual CPU score calculations fall from
39,716,535 to 15,399,303, while cache hits rise from 11,123,295 to 35,440,527.
This is a 61.2% reduction in observed scoring calls, not an equal reduction in
total planner work: rejected trials can be cheap, and other planning work
remains. GPU scores still never select a production plan.

Runtime source and executable hashes match their frozen provenance after all
measurements and comparisons, including the additional 64-track control.

## Validation and limits

- Workspace: 785 passed, 30 ignored. GPU-feature planner tests: 291 passed,
  4 ignored. Trace: 287 passed, 8 ignored. Release planner: 289 passed,
  2 ignored. Two explicit Metal hardware tests pass.
- Envelope tests compare 10,000 varied shapes bit for bit against the scalar
  formula, plus signed zero, adjacent representable boundaries, tiny widths,
  reversed/step fades and extreme values. For arithmetic producing NaN, the
  boundary test checks NaN classification rather than payload identity.
- Frozen original complete-score comparisons cover all three tonalities and
  accepted/rejected shapes. Shared-gate cases are retested after independent
  drum edits. Cache tests compare against uncached scoring, exercise bounded
  eviction, preserve repeated/empty batches, and retain a valid drum repair
  after a shape-specific level rejection. A changed voice fade cannot inherit
  another shape's shared rejection.
- Planner Clippy passes with existing style warnings and no new warning in
  the changed modules. Linux and Intel macOS compile checks pass using rustup
  Rust 1.99.0 and its matching installed target libraries. These do not measure
  Intel runtime performance or prove execution on other devices.
- `./dev.sh --build` refreshes the development bundle without launching it.
  No live library, audio source or running player is modified.

Existing immutable regression analyses supply the measurements. Their reuse
does not newly establish analysis ground truth, search exhaustiveness or
listening quality. No new audio render, listening acceptance or live handoff
deadline measurement is claimed. The existing policy evaluates all directed
pairs and up to four repair rounds through 40 tracks; above 40 it evaluates
twelve successors per track and skips carried-state order repair. Above 200
tracks the rough-cost policy remains unchanged. Times across these boundaries
are not a scaling curve under one exhaustive search policy.

Private evidence remains in `target/planner-production-20261112/`: baseline
profile, intermediate/final frozen executables, source and input hashes,
validation logs, time/RSS records, complete order audits and queue plans.
`validate.py`, `measure.py` and `shadow.py` reproduce the recorded checks.
