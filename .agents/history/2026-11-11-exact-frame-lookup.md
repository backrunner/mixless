# Exact planner frame lookup (2026-11-11)

The user authorized continued optimization only if planner judgment and final
results remain unchanged. This is an execution-path optimization, with no
change to candidates, pruning, score formulas, thresholds, ordering repair,
strategy selection or ties. ORDER_VERSION remains 26.

## Baseline and mechanism

The baseline includes the previous reference-power and sampling optimization
in [the path review](2026-11-11-order-path-review.md). Its source was compared
against the earlier frozen measured source and matched exactly. Both baseline
executables are preserved under `target/order-sampling-20261111/`; this is a
working-tree comparison, not a comparison with Git HEAD.

Repeated source sampling was still a measured hot path. The retained approach:

- Store frame end times in a dense array alongside the scoped immutable frame
  powers. This adds four bytes per source frame plus small per-track metadata.
- Estimate the frame index using the recording span, then verify the exact
  lower-bound conditions. Also check adjacent estimates to accommodate a short
  final frame and floating-point rounding. Use the original partition search
  on the dense end times whenever no bracket is proven.
- Resolve scoped evidence once for a four-sample band-power query. Visit every
  valid sample in the original order, including repeated observations of the
  same frame. Frame start checks, missing-observation handling, power values,
  multiplication, accumulation and division remain unchanged.

For nondecreasing finite end times, the required index i is uniquely defined
by `i == 0 || ends[i-1] <= sec` and `sec < ends[i]`. The adjacent checks prove
these same conditions; the terminal case returns len only when all endpoints
are at or before sec. This is exact indexing, not resampling or quantization.
The estimate is never trusted without validation. Non-finite queries and
invalid/nonmonotonic endpoint arrays take the partition path. Gaps retain the
original `frame.start_sec <= sec` test after lookup.

Reuse remains tied to immutable analysis borrows and shared scoped evidence.
Same-ID foreign tracks use the uncached path, nested scopes restore parents,
and data edits after scope exit cannot reuse an old index. No new work enters
an audio callback. No global source-time cache or synchronization was added.

## Validation contract

- Compare frame indices with the original `StemFrame` partition lookup at
  endpoints and immediately adjacent floating-point values, gaps, short final
  frames, variable intervals, duplicate endpoints, shifted clocks, empty data,
  nonmonotonic inputs and non-finite inputs.
- Compare exact sampled power bits and observation order with direct source
  measurements, with and without a scope and for same-ID foreign evidence.
- Save and compare every audited directed pair plan, carried-state chain plan,
  round repair record and pair matrix during the 11-track Best-order run.
- Compare complete serialized plans for the fixed eleven-track chain, three
  genre excerpts and a pass with stem playback disabled.
- Run the required workspace tests and development bundle build after the
  serial timing comparison. Keep trace-mode evidence tests enabled separately.

Timing inputs are the frozen historical analysis exports with stems from
`target/planner-perf-20261110/`, not a fresh audit of the live library. Before
and after runs are sequential, with builds/tests outside timing runs. Other
applications remain running. The order audit writes diagnostic files in both
runs, so its numbers must not be directly compared with an unaudited earlier
run. Source, binary and input hashes, timings, audit records and complete plans
are retained in the ignored artifact directory. No library/audio was changed.
Offline plan equality does not establish listening or live-device acceptance.

## Results

| Work | Baseline (s) | Optimized (s) |
| --- | ---: | ---: |
| 11-track Best order (planner time) | 387.64 | 362.91 |
| Directed pair evaluation | 70.66 | 62.63 |
| Carried-state repair | 316.98 | 300.28 |
| Fixed 11-track chain (process wall time) | 61.51 | 55.14 |
| Electronic excerpt (process wall time) | 4.94 | 4.49 |
| MUG excerpt (process wall time) | 31.16 | 28.79 |
| DnB excerpt (process wall time) | 10.41 | 9.69 |
| Electronic without stem playback (process wall time) | 0.29 | 0.29 |

Full-order planner time fell 6.4%. Process CPU time
fell from 2060.41 to 1906.62 seconds
(7.5%). These are single local matched observations,
not a guaranteed latency reduction on every library or device. The remaining
chain repair still takes about five minutes on this input.

All 160 Best-order audit files are byte-identical: 110 directed pair plans,
45 distinct carried-state plans, four round records and the pair matrix.
All four repair rounds retain the same costs, corrections and selected best.
The final order remains `[6, 7, 0, 10, 3, 2, 8, 1, 4, 5, 9]`.
All 18 additional serialized plans are byte-identical across the fixed chain,
three genre excerpts and the pass with stem playback disabled (the latter
retains analysis evidence; it is not a missing-stems corpus). The comparison
found zero mismatched files. Request memo hits/misses remain 43/358 in both
full-order runs; the speedup did not come from omitting forecasts.

Six focused source/index/cache tests passed before the optimized run. They
compare against direct source measurements and the original lookup, including
exact power bits, rather than merely asserting a selected musical outcome.

## Integration results

- `cargo test --locked --workspace --exclude mixless-desktop`: 762 passed,
  28 ignored.
- `cargo test --locked -p mixless-mixplan --features trace level::evidence`:
  six passed.
- `./dev.sh --build`: passed, refreshing `target/app/Mixless.app` as
  0.1.0-beta.5, dev. The running application was not restarted.
- `git diff --check`: passed.
- The four frozen input exports still match the recorded SHA-256 hashes.

Tests and builds ran outside the before/after timing suites. Production source
changes are limited to `level.rs`, `level/evidence.rs` and the new
`level/evidence/index.rs`. Existing unrelated working-tree changes are preserved.
The same candidate budgets and planner semantics remain in place; no ordering
cache version bump or live library modification was necessary.
