# Restructured GPU feasibility and planner cleanup

Date: 2026-11-12. Production baseline: `f52d0418949d4623da2531977d7780c9fa3e4276`.
The user requested a better computation layout, a measured GPU decision, and
removal of the planner GPU route if it could not preserve quality and improve
latency. This follows the [production CPU speedup](2026-11-12-planner-production-speed.md).

## Experiment

An isolated detached worktree collected the independent seeds of each
arrangement before width fitting. Exact duplicates were fitted once and
restored to their original positions. A CPU control used the same grouping,
retained the original five ordered trials on each deck, and restored candidate
traversal order before phrasing and final scoring. The alternate arrangement
family remained in its original position. This grouping did not reduce any
search budget or change a musical threshold.

The Metal prototype moved both dependent deck-width rounds into a single
GPU invocation. Its first kernel shared vocal, tonal and foreground rejection
gates across seeds with identical six non-drum fades, then cached those gains
and the reference power. A second kernel evaluated the surviving seeds and
performed both rounds with the existing strict `cost < best - 1e-4` rule.
A dedicated worker combined up to 32 requests within a 100-microsecond window,
reused six buffers and both pipeline states, and submitted both kernels in
one command buffer. Each caller waited once for the complete fit. The raw
Metal mode actually selected the fitted widths; it was not the older shadow
observer with an additional GPU workload on top of CPU fitting.

This follows Apple's guidance on [command-buffer batching](https://developer.apple.com/library/archive/documentation/3DDrawing/Conceptual/MTLBestPracticesGuide/CommandBuffers.html)
and [resource reuse](https://developer.apple.com/documentation/Metal/setting-up-a-command-structure).
Fast math was disabled and `#pragma STDC FP_CONTRACT OFF` was explicit, following
the contraction controls in the [Metal Shading Language specification](https://developer.apple.com/metal/Metal-Shading-Language-Specification.pdf)
(June 2026, page 17). Frame accumulation and trial order remained sequential.
The CPU still performed subsequent phrasing, complete candidate evaluation
and final safety checks.

The maximum initial seed count for one arrangement is 160, from four tonal
anchors, five drum anchors and at most eight voice pairs. Most measured
nonempty groups contained 32–127 unique seeds. The global worker did combine
requests: the largest observed batch in the 29-track verification was 1,263
seeds. Thus this experiment was not limited to dispatching five scalar trials.

## Measurements

Hardware: Apple M5 Max, 18 logical CPUs, macOS 27.2 (26B123). Frozen release
executables used production planner parallelism and immutable regression
analyses with stems. Each invocation started with fresh process caches and
no persistent pair cache. Builds, tests and profiling did not overlap timed
runs; unrelated user workloads remained active. These are individual runs,
not latency distributions or a guarantee for other hardware or libraries.

| Carried-state replay | Production CPU | Regrouped CPU | GPU fitting | GPU plus exact CPU verification |
| --- | ---: | ---: | ---: | ---: |
| Three tracks, two plans | 2.040 s | 1.943 s | 2.011 s | 2.037 s |
| Eleven tracks, thirteen plans | 32.032 s | 33.019 s | 38.000 s | Not run |
| Twenty-nine tracks, thirty-one plans | 57.719 s | Not run | 64.430 s | 66.189 s |

The 11- and 29-track inputs include three repeated tracks to exercise carried
state across a cycle boundary. All final serialized plans in these runs were
byte-identical to the frozen CPU controls. Raw GPU fitting was 18.6% slower
on the 11-track replay and 11.6% slower on the 29-track replay. The small case
was too close to support a useful speed claim. The CPU regrouping also did
not establish a benefit, so it was not retained in production.

The separate cold Best-order comparison used all eleven distinct tracks,
including all 110 directed pair plans and carried-state repair. Both GPU modes
preserved the final order and all 160 audit files byte for byte.

| Cold Best order phase | Production CPU | GPU fitting | GPU only for at least 64 unique seeds |
| --- | ---: | ---: | ---: |
| Pair planning | 50.414 s | 51.601 s | 51.169 s |
| Carried-state repair | 132.486 s | 151.588 s | 147.253 s |
| Whole process | 183.416 s | 203.728 s | 198.518 s |

Phase timers exclude process startup and teardown. The all-batch GPU route
was 11.1% slower overall; avoiding the smaller batches reduced that penalty
to 8.2%, but did not beat the CPU control. These eleven-track order searches
are distinct from the 29-track fixed-order replays; no 29- or 64-track cold
GPU Best-order result is claimed. Total process CPU time was 1,650.73 s for
the CPU control, 1,640.28 s for GPU fitting and 1,646.00 s for the large-batch
hybrid; the small CPU-time reduction did not translate into lower latency.

In the 11-track raw GPU replay, 2,538,045 of 2,625,593 unique seeds (96.7%)
failed a shared gate before full scoring. In the 29-track replay, the ratio
was 4,629,731 / 4,734,128 (97.8%). These are often inexpensive CPU cache hits,
not a large dense scoring matrix. The 29-track GPU path issued 35,632 command
buffers for 60,920 nonempty requests. GPU command timestamps totaled 8.432 s;
host packing/submission/completion service totaled 15.448 s, and caller wait
times summed to 59.351 s. Caller waits overlap across threads and must not be
added to wall time. Regrouping cannot remove the carried-state dependency
between completed transitions or the CPU work outside width fitting.

## Decision preservation

The candidate-order regression compared the original and regrouped CPU
paths bit for bit across all three tonalities and manual/automatic cue cases.
The same fixture also passed with actual Metal execution. These fixtures
alone did not establish equivalence on real inputs.

The 29-track verify mode compared every GPU-fitted shape to its exact CPU
counterpart, while returning CPU fits to subsequent planning. Across 4,734,128
unique seeds it found one different fit. For the saved eight-bar compatible-
key arrangement, the outgoing drum fade began at bar 2: CPU chose an end at
bar 6 (four-bar width), while Metal chose bar 4 (two-bar width). Both ultimately
chose an incoming drum fade ending at bar 6. The final plan did not select this
counterexample, but that does not make the intermediate search equivalent.
The raw mode's zero `different_fits` counter is not a parity result: only the
verify mode performs that comparison.

An independent single-seed Metal replay reproduced the same fit without the
batching queue. A separate release CPU probe reconstructed the frozen frame
bits and reproduced the CPU result. In the first deck round:

| Trial | CPU score | Metal score |
| --- | ---: | ---: |
| Two-bar width | 0.8717209696769714 | 0.8717209696769714 |
| Four-bar width | 0.8716208934783936 | 0.8716210126876831 |

The floating-point cutoff after choosing the two-bar trial is
`0.8716209530830383`. The CPU four-bar score is below it; the GPU score is above
it. A difference of two representable float steps therefore changes the strict
choice despite identical formulas and disabled contraction. The numeric
counterexample is preserved as bits; no empirical score tolerance is treated
as a correctness bound.

The route fails the combined requirement: no measured end-to-end benefit on
these workloads, and no guarantee of preserving planner choices. Keeping the
CPU as an exact authority requires its work as well; weakening thresholds,
accepting a sampled error margin or reducing search coverage was not used to
make GPU results look favorable. This rejects this planner implementation
on the measured device, not GPU acceleration for every possible computation.

## Cleanup and validation

Removed the planner's optional `gpu-audit` feature, Metal shader/backend,
observer queue, comparison metrics/tests, frame packing, scoring hooks,
example wrappers, and its optional `metal`/`objc` dependency edges. Existing
Metal users elsewhere in the application remain intact. Historical reports
are marked as superseded so their old commands are not current instructions.

The production scorer, shared rejection cache, NEON envelopes, seed enumeration,
manual cue rules, tie order and ordering/repair policies remain unchanged.
`ORDER_VERSION` stays 28 and `ANALYSIS_VERSION` stays 27. The experimental wave
implementation was never installed or enabled in the desktop and is not part
of the submitted source.

- Workspace tests: 785 passed, 30 ignored, no failures.
- Release planner tests: 289 passed, 2 ignored. Trace: 284 passed, 8 ignored.
  The three fewer trace tests than the earlier GPU-enabled report are the
  removed GPU diagnostic tests, not removed CPU coverage.
- Planner Clippy across all targets and remaining features passes with existing
  style warnings; the warnings in touched files refer to unchanged loops/ranges.
- Rust formatting and `git diff --check` pass. Locked Cargo checks confirm the
  lockfile only drops the planner's two optional dependency edges.
- `./dev.sh --build` successfully refreshes the development bundle without
  launching it. Other application Metal dependencies still build.
- Four carried-state queues plus the no-stems regression preserve all 88
  complete serialized output plans byte for byte after cleanup. These cover
  13, 24, 31, 16 and 4 plans respectively.
- All 141 frozen experiment source hashes and all four frozen executable
  hashes match their recorded provenance. The isolated numerical diagnostic
  was added after freezing the timing executables and is archived separately.
- Review found no remaining GPU module, feature, environment switch or optional
  Metal dependency in `mixless-mixplan`. The temporary worktree has been removed;
  its archived source and numeric counterexample remain private evidence.

These checks establish preserved plans for the replayed evidence, not fresh
analysis ground truth, exhaustive global search, listening acceptance, or a
live handoff deadline. No source audio or live library was modified, and the
running player was not restarted. The existing search policy boundaries above
40 and 200 tracks remain unchanged.

Private artifacts are under `target/planner-gpu-wave-20261112/`: frozen
executables and hashes, immutable input hashes, replay/order audits, numerical
counterexample, timings/RSS, validation logs, scripts and the archived prototype
source. The temporary source worktree has been removed.
