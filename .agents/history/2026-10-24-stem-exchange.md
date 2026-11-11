# Audible stem exchange and delayed vocal entry

The reported sequence is Dreams → Don't trust me → Where Do We Go in the
persisted 22-track Electronic Best order. The first plan was called StemSwap
because any small difference between two part envelopes counted as independent
mixing. Its nominal 24-bar window also earned duration credit while the incoming
deck was silent and after the outgoing deck had closed. With the original
preceding state, the actual engine held partial selections on both decks for
only 0.160 seconds. The following pair used an end-of-file emergency handoff.

## Changes

- Classification requires both decks to hold an audible proper subset of their
  measured parts for a complete master bar. Source parts below the existing
  relative -40 dB evidence floor do not manufacture an independent selection.
  Open parts must reach half gain; closed parts must be at or below -40 dB gain.
  One full track under a selected layer, unequal fade widths, and small envelope
  offsets remain PhraseBlend. A vocal carry retains its separate semantics.
- The live status uses both decks' current stem controls. The planned technique
  remains in the tooltip; a full-track phase of a planned exchange displays
  Phrase blend rather than Stem swap. Waiting, finishing and pause retain priority.
- Fitting compares the original duration reward with actual source participation.
  Both sources must contribute at least -20 dB of their own full power and of the
  heard sum. This is a conservative participation floor, not a listening metric.
  Silent preparation and finishing no longer earn overlap time. Drum timing also
  searches the adjacent four-bar anchors, alongside the original two-bar anchors.
- Whole-track vocal-entry and exit guards defer only for a prospective fitted
  part arrangement. A successful fit checks the actual vocal entry/release; an
  unsuccessful fit must still pass the original guards. Native-tempo bridges
  retain the existing prohibition on concurrent unrelated rhythmic attacks and
  bass patterns. A multi-bar expanded bridge is no longer treated as a dry cut
  by the pre-render cut-rest test.
- `ORDER_VERSION` is 4. No analysis versions, stored cue semantics or DSP callback
  code change. The renderer now resolves the same audio identity as the app and
  records simultaneous partial-selection time from actual engine snapshots.

## Current evidence

Artifacts are in ignored `target/stem-exchange-20261024/`. The live WAL database
was captured through SQLite backup; the audit did not modify the live database.
All three reported recordings match their stored full-file BLAKE3 hashes. Their
current analyses are version 23 with stem evidence version 2. These recordings
and existing playlists are development/regression material, not a new holdout.

Two comparisons are kept separate:

1. With the original preceding plan held fixed, Dreams → Don't trust me retains
   its source windows, but keeps Dreams' drums while the incoming tonal parts
   take over. Actual simultaneous partial selections increase from 0.160 to
   3.291 seconds. Don't trust me → Where Do We Go changes from a final four-second
   emergency handoff to a fitted 8-bar native-tempo transition, outgoing
   194.413–207.213 s and incoming 32.051–44.851 s.
2. Replaying the entire captured Best sequence with the new planner changes the
   inherited state. Dreams → Don't trust me becomes an 8-bar exchange, outgoing
   173.595–186.395 s and incoming 13.621–26.421 s. Instruments enter first, then
   voice, bass and drums; the incoming first peak arrives complete. The engine
   holds both partial selections for 6.603 seconds. The following transition is
   an 8-bar PhraseBlend, outgoing 90.418–103.218 s and incoming 1.830–15.071 s.
   It completes the first chorus before mixing out and separates the native
   rhythms. Its brief partial-selection phase is not labeled StemSwap.

All renders use the real aligned four-stem cache through the offline 48 kHz
engine, with the plan's inherited rate and pitch. Both pairs complete at the
expected offsets with finite samples below full scale. Final chained-plan peaks
are 0.4024 and 0.4100. These are separate pair renders with correct inherited
states, not a continuous whole-set PCM recording or a listening judgment.

The captured Best sequence plus five original playlist orders cover 94 chained
handoffs. All final plans pass source bounds, non-overlapping incoming/outgoing
windows and carried rate/pitch checks. Emergency/terminal one-bar fallbacks:

| Sequence | Handoffs | Before | After |
| --- | ---: | ---: | ---: |
| Electronic captured Best | 21 | 1 | 0 |
| Downloads | 1 | 0 | 0 |
| DnB | 13 | 2 | 0 |
| Melodic | 10 | 0 | 0 |
| Electronic original order | 21 | 2 | 0 |
| MUG | 28 | 15 | 14 |

These counts exclude nonterminal plans carrying a fallback flag, and do not
establish subjective quality. MUG still has 14 terminal fallbacks. The captured
order was replayed, not reoptimized. No independent listening corpus was added.

Controlled regressions cover simultaneous fades, unequal widths, tiny timing
offsets, one-sided selection, momentary staggering, sustained two-sided selection,
absent vocals and tonal accompaniment, common gain scaling, waiting-time credit,
a muted held-word entry, and rejection when no complete vocal rest exists. Existing voice, peak, key and
tempo regressions remain in place. The generic fitted-beat regression explicitly
requests a beat blend now that unconstrained ranking can prefer LoopOut.

## Final validation

- `cargo test --locked --workspace --exclude mixless-desktop`: 594 passed,
  28 ignored. The focused AutoMix desktop status suite passed all 7 tests.
- The final release audit reproduced all 94 plans byte-for-byte after the last
  absent-accompaniment guard. Previously rendered final pair plans remain valid.
- `./dev.sh --build` completed. The development app reports 0.1.0-beta.5,
  channel `dev`, profile `debug`; this version comes from the concurrent release
  work, not a version bump in this fix. The binary SHA-256 is
  `5c8f08e81090e6ed9ed1d243156182737844c32a2de18433312031cd7942efb3`.
- That exact binary rendered the production toolbar at 1440 logical pixels in
  an isolated offline data directory. `native-status-1440.jpg` captures the
  2880 × 1144 native view: scheduled, waiting, independent Stem swap, finishing,
  paused, and full-track Phrase blend all fit. The isolated preview was closed.
- Changed-file formatting and `git diff --check` pass. Logs, source verification,
  chain checks and binary metadata remain with the ignored artifacts.

macOS: 27.0.1 (26A434). Offline rendering does not use a physical output device.
No human listening or live device playback is claimed. The user's running app
was not restarted; its current playback still uses its previously loaded code.

Follow-up analysis, routing, gain, key display and Best-order changes are recorded in
[the musical-handoffs audit](2026-10-24-musical-handoffs.md). The validation above
remains the evidence for this earlier isolated stage.
