# Batched scoring and Metal comparison

Date: 2026-11-12. Baseline: `86d8fa0863013c91a535416762dd84d2d61c6e78`.
The user authorized the batched scoring backend and CPU/GPU comparison proposed
after the [exact-power investigation](2026-11-11-planner-algorithm-and-routes.md).

## Delivered boundary

Independent width trials now enter one fixed-size scoring batch. The CPU path
still uses the existing scalar formula and exact cache lookups in original
order. Each deck's selected width is reduced with the original strict threshold
before constructing the next deck's batch. Neither candidates, sampled frames,
musical safety gates nor tie rules change. This is a batching interface, not an
enabled SIMD implementation or a faster GPU selection policy.

The optional `gpu-audit` feature supplies a complete Metal counterpart of
`composition::cost`, including tonal/vocal exclusions, foreground absence,
power loss, masking and control-movement cost. Source-only reductions come from
the same immutable CPU preparation. Metal uses safe/precise math through the
macOS-12-compatible compile option. CPU scores alone populate planner caches,
prune trials and choose plans. GPU scores never enter these decisions.

Observations are coalesced into chunks of at most 2048 queries with their
original trial-group boundaries. A dedicated worker joins up to 32 chunks,
shares each immutable frame set within a dispatch, and reuses its Metal buffers
and pipeline. The queue is bounded to 256 chunks; supported observations have
at most 512 frames and 64 trials per group (current planning uses 129 frames
and five width trials). Oversized input or omitted data is reported explicitly.
CPU accesses shared buffers only before submission and after command completion.
Autorelease pools bound per-dispatch Objective-C object lifetimes.

Complete audit mode applies backpressure and drains all captured work before
returning the report. Best-effort mode drops diagnostics when saturated and
marks coverage incomplete. Neither mode alters CPU planning results. Device or
shader initialization failure preserves the CPU result; dispatch errors disable
further GPU work and report uncompared queries. Trace mode starts no GPU work.
Unsupported platforms use the CPU path and return an explicit unsupported status.
The observer is process-wide and intended for isolated offline audit programs;
nested audit calls do not replace it, and panic cleanup restores it.

`ORDER_VERSION` remains 28 and `ANALYSIS_VERSION` remains 27. No persistent
analysis/order semantics, playback DSP, user cue authority or live library
contents change. The development app does not enable `gpu-audit`.

## Running and interpreting the comparison

Both `automix-audit` and `order-benchmark` accept `MIXLESS_GPU_AUDIT` as a report
path when compiled with `--features gpu-audit`. `MIXLESS_GPU_AUDIT_MODE` defaults
to `complete`; `best-effort` is explicitly optional. Without the feature, setting
the report variable returns a clear configuration error instead of silently
claiming a GPU comparison. Without the variable, the normal CPU path runs.

```sh
MIXLESS_GPU_AUDIT=/tmp/mixless-gpu.json cargo run --locked --release \
  -p mixless-mixplan --features gpu-audit --example automix-audit -- \
  analyses.json --stems --json
```

A `complete` report means every observed query was compared, not that CPU and
GPU agree. It separately counts acceptance, nonfinite-result, score-bit,
strict-comparison and batch-selection differences. Up to eight decision
counterexamples retain exact input/shape/score bits for reproduction. Reports
also include cache hits, actual CPU scoring calls, dispatch sizes, GPU time,
packing/submission/wait time and total audited runtime. CPU scoring and queue
submission durations are sums of wall intervals across threads, not exclusive
process CPU time. GPU queries include cached CPU lookups, so GPU kernel time
cannot be directly converted into a whole-planner speedup.

## Evidence and initial result

The first per-call queue was inadequate: the 11-track cycle made 50,839,830
score queries, and best-effort submission omitted 33,048,484. That incomplete
run is retained only as diagnostic evidence. Coalescing and complete mode then
compared all 50,839,830 queries without drops. It found zero acceptance and batch
selection differences, but eight strict pairwise comparison differences and
431,410 score-bit differences. Maximum absolute score difference was about
3.05e-5. The 13 complete output plans were byte-identical to the baseline.

These near-threshold differences are why Metal remains diagnostic. Checking
only its winner or using the observed maximum error as a universal bound would
not establish unchanged candidate coverage. A production GPU selector still
requires conservative error guarantees and CPU handling of every ambiguous
acceptance/ranking decision. No such selector is enabled by this change.

An adjacent preliminary replay took 39.903 seconds on the baseline, 40.008 with
the CPU batching interface and 40.328 with the first comparison queue. The
coalesced complete audit took 39.575 seconds in a later single observation.
These are effectively similar timings with noise, not evidence of GPU speedup.
The GPU is doing additional comparison work while the CPU still plans normally.

The final frozen binaries replayed the regression queues below. Each cycle
appends three existing tracks to exercise carried transition state. These are
sequential carried-state replays, not fresh full Best-order searches.

| Distinct tracks | Output plans | Audited seconds | Compared queries | Different batch winners | Different strict comparisons |
| --- | --- | --- | --- | --- | --- |
| 11 | 13 | 41.774 | 50,839,830 | 0 | 8 |
| 22 | 24 | 51.809 | 65,190,658 | 0 | 0 |
| 29 | 31 | 77.693 | 120,271,108 | 5 | 5 |
| 14 | 16 | 84.682 | 95,417,922 | 0 | 2 |
| 64 | 66 | 332.477 | 357,029,769 | 0 | 23 |
| No-stems regression | 4 | 0.596 | 0 | 0 | 0 |

The six complete plan files matched the previous baseline byte for byte,
covering all 154 output plans. All six reports had complete coverage with no
dropped or uncompared queries. Across 688,749,287 observed queries there were zero acceptance or
nonfinite-result differences, 3,663,089 score-bit differences, five different
batch winners and 38 different strict comparisons. The largest observed
absolute score difference was 0.00164031982421875. These counts include repeated
and cached queries; they are not counts of unique musical cases.

The five batch winner differences in the 29-track queue directly demonstrate
that enabling Metal selection could change planner decisions. CPU selection
kept the final plans unchanged. The reports retain bounded exact numeric
counterexamples, including frames and shapes, for investigating those cases.
The no-stems regression makes no composition-score GPU calls and verifies only
that the CPU fallback plans remain unchanged.

A separate final CPU-only build replayed the 11-track cycle in 41.308 seconds
with the same 13 plans, versus 41.774 seconds with complete GPU comparison.
This single adjacent comparison does not establish a speedup. Runtime source
hashes were checked after all replays against those recorded with the frozen
executables. Pre-analysis was reused unchanged; this work does not newly
validate its musical accuracy or prove globally exhaustive search.

## Validation

- Workspace: 782 passed, 30 ignored. GPU-feature unit tests: 288 passed,
  4 ignored. GPU-feature trace build: 284 passed, 8 ignored.
- Two explicit Metal hardware tests pass: full-score acceptance/selection
  agreement on 180 fixture queries, and observer cleanup after panic/nesting.
  The small fixture tolerance is a regression check, not an error bound for
  enabling GPU decisions. Queue saturation and disconnection also have tests.
- The CPU batching regression checks exact bits against sequential lookups,
  including cache eviction, all tonalities, repeated queries and empty batches.
- Planner Clippy passes with existing style warnings; the new GPU modules add
  no warnings. Linux fallback and Intel macOS cross-compilation pass using
  rustup Rust 1.99.0 with its installed target libraries. Those are compile
  checks, not Linux/Intel runtime or hardware tests.
- Native tests/builds use Homebrew Rust 1.99.1. `./dev.sh --build` passes and
  refreshes the development bundle without launching it. Actual Metal execution
  is on the Apple M5 Max; other GPU vendors and old macOS runtime are untested.

Ignored evidence is under `target/planner-gpu-batch-20261111/`, including frozen
CPU/GPU binaries, source hashes, checks, full plans and complete JSON reports.
The earlier numerical-only prototype is under
`target/planner-gpu-feasibility-20261111/`. Personal analyses and generated
counterexamples remain outside the committed source. No listening acceptance
or new real-audio render is claimed for this batching change.
