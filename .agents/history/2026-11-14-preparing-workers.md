# AutoMix Preparing worker isolation

The reported Still Here → rldwide To ge stall showed a decoded incoming deck
waiting for transition planning while Best was still computing. A sample taken
after AUTO had been turned off showed busy Best workers, but cannot identify the
thread that was blocked during the original incident.

## Confirmed mechanism and change

Best ordering, set forecasts and playback staging all entered the same Rayon
pool. Utility QoS changes which runnable threads receive CPU time; it does not
provide a worker when the shared pool is occupied. Playback planning could queue
behind background work and miss the remaining outgoing audio.

`playback_does_not_wait_for_busy_background_workers` parks every background
worker, submits a nested playback request, then releases and joins all jobs.
The shared-pool baseline failed its two-second completion check. With isolated
workers the same test passed immediately, before any background job was released.

`with_playback_workers` selects a separate lazy pool of at most four workers for
staging and queue replacements, and propagates that selection through nested
jobs. The background pool retains its existing capacity. Candidate searches,
scoring, memo scopes, tie order and planner options are unchanged, so persisted
order semantics and `ORDER_VERSION` are unchanged.

## Validation

- Workspace excluding desktop: 786 passed, 30 ignored
- Desktop: 239 passed, 8 ignored
- `./dev.sh --build`: passed
- The opt-in `audit_chained_playback` uses a SQLite backup, local audio and cached
  stems, and includes the preceding handoff's actual tempo, pitch and established
  entry. The reported pair staged an eight-bar PhraseBlend at outgoing
  167.873–181.302 seconds and incoming 14.901–28.432 seconds. Both handoffs
  completed and rendered PCM was finite and within range.
- Its selected strategy, score and source endpoints match the pre-change replay.
- The old development build also completed this pair when AUTO was enabled near
  the beginning of Still Here. Pair success alone does not establish reproduction
  or resolution of the exact original incident.
- Rebuilt and restarted `target/app/Mixless.app` (dev beta.5, build timestamp
  `2026-11-14T14:12:26Z`). On macOS 27.2.1 with the displayed external-headphone
  output, native playback in explicit Playlist order completed Back Again →
  Still Here → rldwide To ge without manual skips. The latter reached 0:46 and
  the freed deck staged Still Waiting For You. AUTO and transport were then
  stopped, leaving the updated instance running. This run did not preserve the
  original Best-at-61-percent scheduling state, and no listening-quality claim
  follows from observing transport and output meters.

Run the local-audio replay with `MIXLESS_SET_AUDIT_DB` pointing at a backup,
`MIXLESS_SET_AUDIT_TRACKS` containing four comma-separated IDs for previous,
outgoing, incoming and following, and optionally `MIXLESS_SET_AUDIT_STEMS` pointing
at the existing stem cache:

```sh
cargo test --locked -p mixless-desktop audit_chained_playback -- --ignored --nocapture
```

`MIXLESS_SET_AUDIT_PLANS` optionally saves the two complete plans. Local evidence
is under ignored `target/preparing-20261114/`; audio and databases are not source
artifacts. These checks establish scheduling isolation and playback completion,
not subjective listening quality or complete recovery from arbitrary planner/I/O
delays.
