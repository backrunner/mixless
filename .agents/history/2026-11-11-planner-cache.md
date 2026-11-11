# Planner result caches (2026-11-11)

The user asked to reuse calculations that do not need to be repeated. The
task-entry working tree already caches complete Best orders, some evidence and
carried-state chain steps. This adds reuse below a complete playlist and across
the candidate forecasts inside one request. Existing concurrent changes remain
in place. No planner policy or candidate limit changed; `ORDER_VERSION` stays 26.

## Reuse within a request

`mixless-mixplan/src/memo` stores completed `plan_choices` results, including
failed plans, for one Best-order or lookahead request. Nested Rayon jobs share
the completed values. Each thread installs and restores its own guard; input
borrows cover the synchronous worker lifetime. No in-flight result is waited
on: nested work stealing must not deadlock behind another suspended search.
Concurrent cold misses can still compute twice.

The exact key includes both immutable analysis addresses, both complete cue
lists, four rate/pitch bit patterns, all planner options and shortlist mode.
Addresses never survive a request. A same-ID analysis from outside the borrowed
input set cannot read or populate that request's cache. Exhaustive input and
option destructuring forces new fields to be considered. Callers own deep
copies of plans, so later recovery edits cannot alter a cached result.

Retention is bounded to 256 entries and 64 MiB of accounted key/plan buffers;
the byte figure is not total process RSS. FIFO eviction recomputes as necessary
without truncating the search. Oversized results bypass storage. Trace builds
bypass reuse to retain actual candidate rejection collection. Audit builds can
append hit/miss/eviction accounting with `MIXLESS_PLAN_CACHE_STATS`.

## Reuse across playlists and restarts

The desktop ordering worker hashes each track's source identity, complete
analysis, planner options and analysis/order/stem versions once. Ordered pairs
of these fingerprints identify the initial pair score independently of a
playlist's indices. Inserting, removing or reordering tracks therefore retains
valid old pair scores. Changed inputs produce misses. A-to-B and B-to-A remain
distinct.

The library stores exact finite nonnegative `f32` bits, bounded to 20,000 rows
by recent use. Schema 5 adds `pair_costs` and its recency index; the schema 4
upgrade is additive. Downgrade reconstruction treats this table as disposable
derived data and clears it with the whole-order cache. Invalid stored values
are discarded. Database failures fall back to calculation.

The host batch-loads scores before parallel evaluation, then workers read the
immutable matrix and collect new scores in memory. A completed stable request
flushes newly measured scores in one transaction. Cancelled requests do not
publish partial orders or flush their unfinished work. Whole-order cache hits
still bypass the search entirely. Initial pair-cache hits preserve progress
accounting and still run the complete carried-state chain repair; corrections
from that chain are never written into the cold pair table. Pair prefetch is
bounded to sets of at most 200 tracks; larger sets use the existing rough-cost
ordering path.

## Evidence and reproduction

Ignored `target/planner-cache-20261111/` holds task-entry/final source snapshots,
immutable executables, input/binary hashes, results and logs. Inputs are frozen
analysis exports with stems from `target/planner-perf-20261110/`, not fresh
measurements of today's library. No live library or audio was modified.

The task-entry and request-cache executables both use ordering version 26.
The earlier `planner-perf-20261111` comparison used version 25 and cannot be a
speed baseline for this change.
`before-src` and `after-src` identify those frozen executables. `after-*` was
built before the persistent pair layer and the later avoidance of copying
oversized plans. `final-src` and `final-source-sha256.json` record the final
implementation used by the cold/warm benchmark and integration checks.

- Three-track Best order: 44.620 s before, 43.634 s with request reuse; both
  produce `[0,1,2]`. There were 8 hits and 45 misses, retaining about 8.1 MiB.
  This is a small single-run difference, not a demonstrated broad speedup.
- Electronic, MUG and DnB excerpts: all six complete serialized plans are
  byte-identical before/after. Runner times were 5.604/6.513, 39.839/38.577 and
  13.074/13.068 seconds respectively. These do not establish consistent gains.
- Eleven-track request-cache run completed in 456.284 s, including 93.024 s
  pair evaluation and 363.258 s chain repair. It reused 43 plans, missed 358,
  evicted 102 and retained 256 entries; peak accounted buffers were 51,688,924
  bytes. Its order was `[6,7,0,10,3,2,8,1,4,5,9]`. There is no matched version-26
  uncached eleven-track timing here, so this is completion/cache evidence only.

Other applications remained active. The after timing suite ran before the
workspace tests and builds. Timing is local offline evidence, not a real-time
playback or listening acceptance claim. First-time chain repair remains the
dominant work; pair caching mainly helps overlapping subsequent playlists.

The offline `pair-cache-benchmark` runs cold and warm directional pair scores
while retaining complete chain repair, and asserts identical final order:

```sh
rtk cargo build --locked --release -p mixless-mixplan --features audit --example pair-cache-benchmark
rtk proxy target/release/examples/pair-cache-benchmark ANALYSES.json --stems
```

On the three-track input, the final executable measured all six directed pairs
on the cold pass and reused all six on the warm pass:

| Final-code pass | Pair phase (s) | Complete Best order (s) | Recomputed pairs |
| --- | ---: | ---: | ---: |
| Cold | 4.479521 | 35.815164 | 6 |
| Warm | 0.000155 | 30.821820 | 0 |

Both returned `[0,1,2]`. Each pass still recorded eight request-cache hits;
misses fell from 45 to 39 when the six cold pairs were skipped. This isolates
the planner's warm pair reuse in memory; it does not time SQLite loading or a
native UI request. Disk reopen and changed-playlist reuse are covered separately
by library/desktop integration tests. The complete reduction is about 13.9%
in this single observation, with remaining work dominated by chain repair.

## Integration validation

- Workspace tests excluding desktop: 758 passed, 28 ignored.
- Desktop tests: 239 passed, 7 ignored.
- Trace-mode memo tests: 2 passed.
- `./dev.sh --build` passed and refreshed `target/app/Mixless.app`; build-info
  reports 0.1.0-beta.5, dev, revision `v0.1.0-beta.5-1-g9651628-dirty`. The
  running app was not restarted. The pair benchmark release build passed too.
- Coverage includes exact input keys, successes/failures, nested/thread scopes,
  edits after scope exit, bounded eviction, oversized-plan bypass, unchanged
  warm-cache ordering/progress, invalid score recomputation, migration/reopen,
  exact float bits, retention, directional reuse after playlist changes and
  discarding newly learned scores when a host request is abandoned.
- `git diff --check` passed.

Initial validation caught a test-only SQLite `usize` conversion; it was changed
to `i64` before the complete successful runs above. Current-tree integration
includes the unrelated concurrent work, while frozen executables identify the
scope of the performance observations.
