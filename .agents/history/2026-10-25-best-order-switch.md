# Switching away from an unfinished Best order

## Change

An explicit ORDER/SHUFFLE choice now publishes the source queue directly,
without reading or waiting for the Best result cache. In-progress Best work
can continue as background cache warming. Intent and result publication share
the selection lock, so a finishing worker cannot replace a newer mode/source
choice. Returning explicitly to Best can reuse the completed result.

Leaving Best also polls a pending AUTO startup immediately: the old Best
waiting state and percentage are released, while genuinely unfinished track
analysis remains a readiness requirement. The active source still comes from
`mix_playlist()`/`automix_tracks`, independently of visible playlist browsing.
The current engine handoff remains owned by the existing preparation logic.
No planner/scoring semantics or ORDER_VERSION changed.

## Verification

- Added a controlled contention regression. Holding the Best result cache
  made the previous implementation fail its one-second switch deadline; the
  fixed implementation switches without releasing that cache first.
- Added GPUI mouse-click coverage for BEST → ORDER → SHUFFLE with displayed
  calculation progress, plus tests for late cache completion, source changes
  and immediate release of startup progress. Five new regressions passed.
- Full desktop suite: 216 passed, 7 ignored.
- Workspace excluding desktop: 642 passed, 28 ignored across 29 suites.
- `./dev.sh --build`, changed-file rustfmt checks and `git diff --check` passed.
- Raw before/after, full-suite and build logs are under ignored
  `target/best-order-switch-*.log`.

Initial native attempts returned `cgWindowNotFound`. After the user confirmed
the session was unlocked, the current built debug preview opened successfully
on macOS 27.0.1 (26A434), with an independent bundle identity, isolated temporary
data, MIDI/deep analysis disabled, and offline audio/models. Real mouse clicks
cycled ORDER → SHUFFLE → BEST → ORDER. Original Retina captures are in
`assets/screenshots/order-switch/`, with build/capture metadata in
`assets/screenshots/capture.json`.

This native preview uses synthetic toolbar snapshots and verifies OS mouse
delivery and rendered modes. The controlled in-progress worker, cache-lock
contention and late-result races remain covered by the GPUI/core regressions;
the preview does not establish a real optimizer timing or playback result.
The later combined suites passed 219 desktop and 643 workspace tests, with
7/28 ignored; the development build passed. The owned preview was stopped.
Live playback was not restarted, and live library/audio data was not used.
