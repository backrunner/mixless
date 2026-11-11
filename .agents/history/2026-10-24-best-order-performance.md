# Best order performance and nonblocking AUTO startup (2026-10-24)

AUTO now checks the valid Best cache before choosing its default. A cache miss
releases startup in playlist order before the expensive optimizer begins. That
session remains in ORDER when the background result arrives. The result is
persisted and can be used on the next AUTO start or by explicitly choosing BEST.
Explicit ORDER, BEST and SHUFFLE choices remain authoritative. Analysis readiness
is still required; this change removes the ordering-search gate, not analysis.

Pair evaluations use a shared atomic work queue across available cores instead
of fixed chunks. Independent start tours and the three lookahead forecasts also
run concurrently, with deterministic reduction in their original order. Carried
entry/rate/pitch dependencies across successive handoffs remain sequential.

Track energy for rough and pair scoring is computed once per track per request. Repair rounds memoize exact
three-track forecasts including bitwise entry, earliest exit, rate and pitch.
The memo is local to immutable inputs and bounded by four times the track count.
Incoming bounds and reserve checks precede clock integration; curve buffers are
reserved once, unused blend bridge-grid checks are skipped, and constant logarithms
are hoisted. Candidate budgets, scoring and tie rules are unchanged. ORDER_VERSION
remains 6 because this optimization preserves computed orders and plans.

The desktop ordering thread uses Darwin utility QoS. Rust pthreads were measured
to reset a utility parent's QoS (17) to default (21); scoped workers now explicitly
carry the parent's scheduling class, including nested lookahead workers. Superseded
playlist/evidence requests stop claiming work and finish their in-flight forecasts
without publishing a partial order. No ordering work runs in the audio callback.

## Measurements

Local release builds on macOS 27.0.1 (26A434), Mac17,14, 18 logical CPUs. Baseline
is the existing uncommitted source at task entry, not git HEAD. Inputs are the
same immutable analysis exports with harmonic key shift and stem playback enabled.
Other applications remained running; these are local wall-clock observations,
not an isolated hardware benchmark or a playback underrun guarantee.

The final pass alternated baseline and final optimized executables for each set:

| Tracks | Before (seconds) | After (seconds) | Speedup |
| --- | --- | --- | --- |
| 22 | 78.74 | 38.21 | 2.06x |
| 29 | 31.00 | 13.11 | 2.36x |
| 14 | 36.64 | 18.18 | 2.02x |
| 11 | 18.00 | 8.90 | 2.02x |

The earlier pass also produced identical permutations, with speedups of
1.92x, 2.06x, 2.22x and 2.48x across the same four nontrivial sets. Five exports
including a two-track set produced 73 byte-identical fixed-order chained plans
between a reference planner and the optimized planner, without trace mode.
The comparison covers source positions, clock and automation curves, lookahead,
carried offsets and summary scores. It does not claim new listening evidence.

Inputs, executable hashes, both measurement passes and full plan comparisons are
in ignored `target/order-perf-20261024/`. `manifest.json` fingerprints the inputs
and binaries. The reproducible timing entry is:

```sh
rtk cargo run --release --locked -p mixless-mixplan --example order-benchmark -- ANALYSES.json --stems
```

## Verification

- Workspace tests excluding desktop: 616 passed, 28 ignored.
- Final planner tests including cancellation and inherited utility QoS: 182 passed,
  one ignored.
- Full desktop tests including cache-hit startup, cache-miss startup, background
  completion without opt-in, next-start reuse and explicit choices: 194 passed,
  seven ignored.
- `rtk proxy ./dev.sh --build` succeeded and refreshed the development app bundle.
- `rtk git diff --check` passed.

The running app was not relaunched, and no live library or audio was modified by
the benchmarks. No new native playback/listening or physical-device validation
was performed. Existing unrelated working-tree changes were retained.
