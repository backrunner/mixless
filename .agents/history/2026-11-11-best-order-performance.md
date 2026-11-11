# Best order computation (2026-11-11)

The baseline is the dirty working tree at task entry, not Git HEAD. Existing
planner, analysis, desktop and dependency work is preserved. Inputs are the
frozen analysis exports from `target/planner-perf-20261110/`, including stem
evidence. They are historical snapshots, not a fresh audit of today's library.
No library data or audio was changed.

## Changes

- Prepare vocal source positions, their wall times and the regular control
  grid once per arrangement, then share them across all candidate renderings.
  Each shape still selects its own audible vocal positions and fade endpoints.
- Prepare exact linear powers of stem frames once per immutable pair scope.
  Nested scopes reuse the corresponding arrays and workers share them through
  `Arc`. Thread-local guards stay on their thread and restore the previous
  scope. Track addresses are scoped to immutable borrows; track IDs are not
  cache keys. Frame selection and floating-point accumulation order stay the
  same. Foreign tracks and calls without a scope compute directly.

Candidate budgets, safety checks, musical scores and chain-repair rounds are
unchanged. The frozen performance comparison uses task-entry `ORDER_VERSION`
25. Concurrent musical-policy work subsequently moved the working tree to 26;
that work is preserved and is not part of these timing comparisons. No GPU backend or
approximate energy calculation was introduced. Chain steps retain their real
entry, earliest departure, rate and pitch dependencies.

## Reproduction

Ignored `target/planner-perf-20261111/` contains task-entry source, immutable
before/after executables, input and executable hashes, macOS sample captures,
timings and full plans. Build with `audit`, without `trace`, to exercise the
production parallel path:

```sh
rtk cargo build --release --locked -p mixless-mixplan --features audit --example order-benchmark --example automix-audit
rtk proxy target/release/examples/order-benchmark ANALYSES.json --stems
rtk proxy env AUDIT_PLANS_OUT=plans.json target/release/examples/automix-audit ANALYSES.json --stems --json
```

Measurements use macOS 27.2 on Apple Silicon with 18 logical CPUs. Before and
after planning runs are sequential. Other applications remain active; short
builds and sampling overlapped parts of the initial baseline. These are local
offline measurements, not isolated timing guarantees, listening acceptance or
playback deadline tests. Tests and the development bundle build are scheduled
after the planning comparisons.

## Rejected scheduling experiment

Counting active searches to shrink speculative batches regressed the pair phase
from 91.30 to 102.07 seconds. The complete run was stopped after exceeding the
completed baseline. Suspended parent searches on the nested work-stealing pool
also contribute to that count; it does not measure runnable work or spare cores.
The counter, batching change and its test were removed. Final production
scheduling is unchanged. `after-*` artifacts describe this rejected experiment;
`shared-*` describe the retained data-reuse implementation.

## Integration checks

- `cargo test --locked --workspace --exclude mixless-desktop`: 750 passed,
  28 ignored, including shared-power frame selection, worker/nested-scope
  isolation, identical IDs with different evidence and edits after scope exit.
- `cargo check --locked -p mixless-mixplan --all-targets --features audit,trace`:
  passed.
- `./dev.sh --build`: passed. The refreshed bundle reports 0.1.0-beta.5, dev,
  revision `v0.1.0-beta.5-1-g9651628-dirty`. The running app was not restarted.
- `git diff --check`: passed.

These integration checks include concurrent working-tree changes. Performance
executables were built before those later musical-policy edits and remain
immutable. This separates the performance comparison from current integration.

## Shared-data measurements

| Work | Task entry (s) | Shared data (s) |
| --- | ---: | ---: |
| 11-track Best order | 561.58 | 538.21 |
| Its 110 directed pairs | 91.30 | 87.98 |
| Its carried-state chain | 470.28 | 450.23 |
| Electronic excerpt, two transitions | 6.89 | 6.26 |
| MUG excerpt, two transitions | 59.25 | 45.31 |
| DnB excerpt, two transitions | 19.40 | 16.64 |

The complete permutation remains `[0,2,8,1,10,3,4,5,9,6,7]`. All six excerpt
plans are byte-identical. The full-order reduction is only 4.2% in this single
comparison, not a substantial latency fix. The tour-selection phase itself
took less than a millisecond; repeated transition/arrangement searches dominate.
Several Godot processes and training were concurrently consuming CPU, so small
wall-time differences must not be interpreted as a stable guaranteed speedup.

## Rejected fade-sample cache

A second experiment cached each individual fade curve across shape costs.
3,300 synthetic shape/tonality combinations matched the reference cost bits,
including rejection and eviction. Complete Best order remained identical but
took 533.25 seconds versus 538.21 for shared data alone; CPU time fell only
2513.19 to 2490.11 seconds. Its pair phase regressed to 95.01 seconds. The
additional memory and cache machinery are not justified by a less-than-1%
whole-order difference on this loaded machine, so the change and its dedicated
test were removed. `final-*` executables/timings and `frozen/` preserve this
experiment, not the retained implementation. `shared-*` remain the retained
performance reference. Final integration checks run against the retained code.

After removing both experiments, planner tests passed again (262 passed,
1 ignored), the audit/trace all-targets check passed, and `./dev.sh --build`
refreshed the development bundle successfully. No experiments remain in the
production code. The original shared-data edits and their two isolation tests
are the retained implementation.
