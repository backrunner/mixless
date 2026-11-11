# AutoMix planner performance (2026-11-10)

Baseline is the uncommitted checkout at task entry, not Git HEAD. Existing
musical-policy, analysis and desktop changes are retained. The frozen inputs
come from a read-only SQLite backup of the current library: four playlists,
11/22/29/14 entries, analysis version 26, all with stem evidence. No audio was
reanalyzed and no live library records were changed.

## Changes

- Memoize exact part-shape costs within each arrangement, including rejected
  shapes. Use a fixed-size bitwise shape key instead of allocating a vector
  for every deduplication lookup. Bound the cost cache to 4096 entries.
- Reuse structural and note-based melody-window searches across arrangement
  seeds. The cache borrows both immutable source clocks and both analyses;
  exact desired-window bits and the evidence kind distinguish queries.
  At 1024 entries it evicts cached values, never musical candidates.
- Memoize vocal continuation at exact source-time bits within an immutable
  planning call. Nested scopes restore their parent, foreign tracks bypass
  the cache, and the guard cannot move between threads. At 32768 entries,
  recomputation replaces growth; no observations are rounded or discarded.
- Forecast independent following-track departures concurrently after the
  first complete two-transition cost is known. Deduplicate carried states,
  preserve the original candidate reduction and tie order, and propagate
  caller QoS with the existing worker helper. Work remains bounded by the
  existing entrance shortlist. Speculation can evaluate a candidate which a
  later, better result would have pruned in the serial implementation.
- Optimize `mixless-protocol` at level 2 in development builds, matching the
  planner's existing setting. Shared curve sampling and evidence computation
  otherwise ran without optimization in the development app.

Planner search budgets, scoring, safety gates, analysis payloads and ordering
cache keys are unchanged. Dependency API adaptations are described in
[the dependency report](2026-11-10-rust-dependencies.md). `ORDER_VERSION` remains
24 because the changes preserve plan/order semantics. Temporary memoization
does not survive edits or planning calls.

## Shared multicore execution

A single reusable Rayon pool has one worker per available logical CPU (18 on
this machine). Pair/starting-tour jobs, incoming entrance shortlists, current
native/held alternatives and following departures share that pool. Nested
work steals existing workers instead of creating more OS threads. Each job
inherits caller macOS QoS and restores the previous class after nested work.
An unspecified caller explicitly uses default QoS, preventing a reused worker
from retaining an earlier background job's priority; guards stay on their thread.
Background planning stays subordinate to playback scheduling.

Each entrance retains its own original score bound and exit/mode/length
traversal. Equal scores reduce by their original traversal ordinal. Expensive
routed arrangement checks run in bounded batches that grow from 1 to at most
twice the worker count. Ordered reduction rechecks the updated serial bound,
so speculative results cannot introduce candidates the serial search pruned.
Whole-track peak windows, instrument boundaries and audible reference powers
are prepared once and shared as immutable evidence across worker jobs. Workers
establish and restore their own scopes with shared evidence; thread-local
guards are never sent across Rayon jobs. This avoids recomputing whole-track
evidence for every small arrangement batch. The `trace` path retains
serial evaluation for rejection collectors.

## GPU assessment

The macOS sampling captures show arrangement search, vocal continuation and
melodic-window work dominating real forecasts. Best order already parallelizes
pair evaluations. The added concurrency targets independent future departures,
which previously ran serially after current-transition forecasts.

No Metal backend was added or benchmarked. A future experiment should batch
independent shape scores and measure total CPU-to-GPU submission/synchronization
latency, not only shader time. Branch-heavy, early-exit search and carried
transition dependencies cannot simply be replaced by a dense matrix operation.
Apple describes SIMD branch divergence in
[Get to know Metal function pointers](https://developer.apple.com/videos/play/wwdc2020/10013/)
and grid execution in
[Performing calculations on a GPU](https://developer.apple.com/documentation/metal/performing-calculations-on-a-gpu).
This is an architectural assessment, not evidence that a GPU version is slower.

## Reproduction and evidence boundaries

Ignored `target/planner-perf-20261110/` contains the immutable SQLite copy,
analysis exports, pre-change source snapshots, baseline/optimized executables,
SHA-256 manifest, macOS sampling captures, full plans and timing results.
Examples run with `audit`, without `trace`, to retain production parallelism:

```sh
rtk cargo build --release --locked -p mixless-mixplan --features audit --example order-benchmark --example automix-audit
rtk proxy target/release/examples/order-benchmark ANALYSES.json --stems
rtk proxy env AUDIT_PLANS_OUT=plans.json target/release/examples/automix-audit ANALYSES.json --stems --json
```

Measurements use macOS 27.2 (26B123), Mac17,14, 18 logical CPUs. Other
applications, including the existing development app, remained running.
Offline planning comparisons do not establish listening quality or playback
deadline/underrun performance. The app bundle is rebuilt without interrupting
the running instance; applying it requires the next application restart.

## Validation

- Final workspace tests excluding desktop: 727 passed, 28 ignored. This includes planner regressions, exact cached-cost/scoped-evidence
  isolation, stem codecs and portable package roundtrips. Nested-pool, QoS restoration, shared instrument-evidence and shared
  peak-evidence regressions are included.
- Desktop tests: 233 passed, 7 ignored. Seven-format synthetic import, analysis
  and offline deck playback test explicitly enabled: 1 passed.
- `cargo check --locked --workspace --all-targets` passed.
- `./dev.sh --build` passed. Bundle `--build-info` reports 0.1.0-beta.5, dev,
  revision `v0.1.0-beta.5-1-g9651628-dirty`.
- `git diff --check` passed.

## Bounded measurements

The initial 11-track full-order experiments were stopped after more than 1224
seconds (baseline) and 685 seconds (first optimization). Neither completed;
they overlapped and are not a speedup measurement. The three-track comparison
uses the same first three entries of that export:

| Work | Task-entry seconds | Shared-evidence seconds | Output |
| --- | ---: | ---: | --- |
| Three-track Best order | 185.42 | 69.72 | Identical permutation |
| Its pair-cost phase | 11.74 | 4.87 | Same inputs and candidate budget |
| Its carried-state chain phase | 173.68 | 64.86 | Same final permutation |

Fixed-order planning with following-track anticipation:

| Input | Transitions | Task-entry seconds | Shared-evidence seconds |
| --- | ---: | ---: | ---: |
| 11-track Melodic Dubstep chain | 10 | 494.52 | 115.61 |
| Electronic excerpt | 2 | 11.52 | 5.73 |
| MUG excerpt | 2 | 120.34 | 49.44 |
| DnB excerpt | 2 | 37.13 | 24.79 |

All 16 complete serialized plans are byte-for-byte identical to task entry,
including source clocks, automation and mix-point choices. The shared evidence
stage reduced the 11-track chain's CPU time from 1125.46 to 564.47 seconds
relative to the intermediate multicore implementation. Its wall time dropped
from 198.69 to 115.61 seconds. Average CPU usage over the entire chain was 4.88
cores. The full 11-track order run sampled roughly 1180% process CPU during
pair evaluation. An 18-worker pool does not imply that sequential work or dependencies
can use all cores throughout the run.

These are single local observations with other applications running, not an
isolated benchmark or a whole-library guarantee. Earlier runs also overlapped
other offline planning/build work. The final shared-evidence measurements ran
without this task's concurrent builds/tests. Input/binary hashes, all
intermediate measurements and full plans are preserved. The final QoS fix only
changes unspecified-thread scheduling; it leaves planning arithmetic intact.
No subjective listening or live scheduling-deadline acceptance is claimed.

shold was changed.
The catalogue Macro effect p99 was 0.1274 ms versus a 0.12 ms limit;
cue p99 was 15.184 ms versus a 1.333 ms limit; 512-frame recording p99 was
11.485 ms versus a 5.333 ms limit. Main-output cases and stems passed.
Other system workloads included training and an application smoke test. This
is neither an isolated regression attribution nor realtime acceptance. There
was no physical-device listening session. A quiet-machine matched baseline
is required to attribute these scheduling-tail failures.


A subsequent serial matched-engine run (before/after dependency changes) gave
5/5 passing before and 4/5 after: only the 128-frame cue headroom check failed,
at p99 1.411 ms against its 1.333 ms threshold. Three further alternating cue
runs gave 3/3 before and 2/3 after, with final p99 1.467 ms; two after-upgrade
runs passed at 1.069 and 1.172 ms. Median times were approximately 0.88 ms in
both binaries. These samples leave an intermittent tail/headroom issue open;
they do not prove a dependency regression or justify declaring it resolved.
The threshold reserves half the device period; exceeding it is not itself
proof of a missed device deadline. No test budget was relaxed. See
`before-deps-callback-budget.log`, `after-deps-callback-budget.log` and
`*-cue-repeat-*.log` in the artifact directory.


## Final full-order bound

The final 11-track Best order run was stopped by its 300-second subprocess
limit. The rough phase ended at 0.0034 seconds and all 110 directed pair costs
at 76.5953 seconds; tour selection finished at 76.5956 seconds. Carried-state
chain repair did not finish within the remaining 223.4 seconds. No permutation
was emitted and there is no completed full-order baseline to compare. Full
playlist Best order therefore remains a latency limitation despite the measured
three-track and transition-chain gains. This run uses the final default-QoS
restoration fix and records its binary SHA-256 in
`target/planner-perf-20261110/verified-order-11-run.json`.
