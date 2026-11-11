# AutoMix phrase integrity and persistent previews

The reported BEST listening problems came from a build preceding the previous
conversation's changes. Its exact executable and cut timestamps are unknown.
The stored old BEST order matches the v9 audit order. Replay v9, the current-turn
v10 baseline, and v11 on the same frozen v26 analyses; do not describe this as
an exact capture of the user's old session.

## Implemented

- Keep acknowledged IN/OUT after an engine handoff and the next deck reset.
  A reset can mark the completed plan modified; it must not hide the entire
  forecast. Manual takeover of an armed plan still invalidates its display.
- Retain the completed entrance by exact playlist occurrence, including repeated
  tracks and commits before the first forecast. Display history never becomes
  an executable stale plan.
- Protect short rests within a sung episode using a local, half/double-invariant
  clock. Check both outgoing removal and incoming reveal through the complete
  stem/fader/gain/EQ/filter routing.
- Reserve channel EQ restoration before heard voice, including sustained notes
  missed by the activity detector. Coordinate the incoming melodic window with
  the fitted outgoing release. Reject a whole bar of completely absent melodic
  beds unless another real foreground carries it; natural silence remains valid.
- Offer validated internal vocal releases to the short-handoff search. Preserve
  peak, note, manual-cue and final-routing safety checks. ORDER_VERSION is 11.
- Add observer v4's independent continuity risk index and component diagnostics.
  It reads source analyses and final controls, not planner candidate scores.
  Keep 1/2/3-pulse sensitivity and rhythm/harmony/energy/pitch/overlap/waiting
  measurements alongside it. It is not an overall listening-quality predictor.

## Evidence

Ignored local evidence: `target/automix-phrases-20261026/report.md`,
`aggregate.json`, `provenance.json`, `validation.json`, and per-set JSON/WAV files.
Final planner/output prefix is `held`; earlier `candidate`, `fitted`, `final`,
`release`, `complete`, and `verified` files are intermediate experiments.

All four saved playlists were checked (76 tracks; 80 transitions per mode,
including the cycle closure and one repeated inherited edge). Full-file BLAKE3
matches the database and analysis record for all 76 sources. The other 27 library
entries and hidden folders are outside this playlist matrix. No live DB edits,
new analysis fields, audio callback changes, or track-specific production rules.
The corpus was previously inspected and is regression material, not a holdout.

For fixed ORDER plus fixed v10 BEST, 154 of 160 transitions are observable at
both revisions. Default two-pulse vocal-damage detections fall from 64 to 0.
This does **not** pass full musical acceptance:

- The final three-pulse grouping still flags 14 fixed-sequence transitions.
- Played-out transitions increase from 26 to 65; actual >=4-bar audible overlaps
  fall from 24 to 13. Fixed ORDER alone increases from 17 to 37 played-out tracks.
- Fresh DnB BEST has an index mean of 31.70 versus v10's 33.79, instrumental-bed
  gap exposure of 36.05 versus 34.30 seconds, and three versus one played-out
  transitions. This is a documented regression, not hidden in a global average.
- The three reported final handoffs still score zero on the conservative index
  because of instrumental-gap or abrupt-cut components, despite no default
  vocal-episode damage detections. Some carried-vocal passages and deliberate
  structural cuts may legitimately trigger those components; listening and
  annotations are required before treating them as musical truth.

Lost & Found -> Feel This Good's measured bed gap changes from 24.30 seconds
in the v9 replay to 4.85 seconds. The v9 We Are The Energy exit removes part of
an exposed vocal episode. At the v9 In The Echo exit, current analysis does not
reproduce an outgoing vocal cut; it instead detects Underwater entering in the
middle of a vocal episode. Preserve this distinction from the user's report.

Final plans were rendered through the real four-stem engine for 39 handoffs,
6021.46 seconds, including a variable-tempo playlist in ORDER and BEST. A v9
comparison adds 15 handoffs and 2015.20 seconds. All complete with finite samples
and peak <=0.666. Six PCM16 comparison clips include eight-second handles.
Render input plans match final plans field-for-field. No subjective listening
or physical audio-device acceptance was performed.

Validation: workspace 652 passed/28 ignored, desktop 233 passed/7 ignored,
observer 31 passed; development bundle, changed-file rustfmt and diff checks pass.
One acquisition test timed out during four simultaneous BEST computations; the
complete workspace rerun passed after that CPU load ended. No test was weakened.
Desktop coverage includes real engine completion/reset, cold previews, duplicates,
pending/late queue acknowledgements, switching, full-list scroll and menu states.

Native acceptance remains blocked: repeated CUA discovery returns
`Sky Computer Use native pipe startup failed`. No current native screenshots,
listening verdict, frame-time claim, commit, push, or publication.
