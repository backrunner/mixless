# Harmonic transition matching — 2026-10-17

The existing pending work was reviewed, corrected, validated and pushed as
`4b5671cead8586cf36a3a7509ba8221f40c0b474` before this change. Its Rust and Website
workflows passed. Review corrections covered FX restoration overwriting manual
edits, terminal analysis failures leaving AutoMix preparing, the controller
schema, and preventing repeated stem weighting in the audit tool.

## Mechanism and change

The engine already supports pitch shifting independently of tempo, including
aligned stem playback. Planning only allowed limited pitch correction for
some blends, while Vocal Ride could accept local note consonance without a
reliable harmonic match. Some catalog plans also returned pitch to zero during
the audible overlap. These paths could put an outgoing vocal over an unrelated
incoming key.

The shared matcher now uses both decks' sounding pitches and the actual overlap
chroma. It selects the nearest compatible incoming pitch within six semitones
of its source, includes fine tuning carried from unlocked tempo, and retains
that pitch through the handoff and into the next plan. Relative major/minor
ambiguity is treated as one harmonic family; competing keys still reduce
confidence, and the confidence threshold remains 0.55. Cached track analysis
and its version are unchanged.

Vocal Ride requires reliable harmony and checks sung notes after both pitch
offsets. Bridges, loops, emergency handoffs and effect tails also check harmony.
When evidence cannot support a match, stem transitions separate outgoing and
incoming pitched material; uncertain tonal emergency handoffs use a dry cut.
An already selected Echo/Reverb cannot bypass the tail check. No track titles,
IDs or genre-specific thresholds are used by production matching.

Deck and top-waveform labels show the sounding Camelot key, including cents
for fractional tuning. The library continues to show the source key.

## Real-library evidence

A fresh SQLite backup supplied analysis version 20 and stem evidence version 2.
The four source files were BLAKE3-verified against their cached analysis hashes.
The separator model hash starts `d05c269d0178d2a7`; the note model starts
`2c3c1d144bfa61ad`. Existing stem-weighted analysis was not weighted again.

| Pair | Selected transition | Incoming pitch | Rendered duration | PCM peak |
| --- | --- | ---: | ---: | ---: |
| Ghost Voices → Platinum | Vocal Ride, 8 bars | -3 semitones | 24.00 s | 0.4189 |
| Particle Arts (44.1 kHz) → Platinum | Echo Out, 4 bars | -1 semitone | 13.52 s | 0.3432 |
| Particle Arts (48 kHz) → Platinum | Phrase Blend, 8 bars | -1 semitone | 18.97 s | 0.3353 |

Ghost Voices remains 10A; Platinum changes from 7B to compatible 10B. On the
same selected Vocal Ride window, the mean sung-note dissonance measure fell
from 0.3766 to 0.2297 across 120 observations. This metric is not a listening
quality score. The previous planner selected a 25-bar, unshifted Vocal Ride.

All three real-source renders completed their engine handoffs with finite,
in-range PCM and the expected final pitch/rate. The first two exercised cached
four-stem playback; the third used whole-track playback because its stem gains
were unity. A continuous three-track render completed both handoffs in 556.55 s,
peak 0.43696, with pitch -3 carried into Platinum's following transition.

Five playlists supplied 72 adjacent transition checks, plus 28 checks in the
MUG BEST order. Final plans passed entrance/exit nonoverlap, carried-offset,
constant-pitch and six-semitone-bound checks. Conservative fallback remains
available; these checks do not claim every pair produces a long blend.

## Validation and boundaries

- Workspace suite: 527 passed, zero failed, 26 ignored. After adding two more
  catalog/tail regressions, all 130 mixplan tests passed.
- Desktop suite: 185 passed, zero failed, seven ignored.
- Controlled regressions include every pair of 24 Camelot keys with integral
  and fractional carried pitches, transposition round trips, shifted sung-note
  comparisons, unknown-key fallback, stem separation and stable catalog pitch.
- Release callback budgets: five passed, run serially without concurrent
  builds/tests. Dual-deck stems, pitch, FX, recording and cues measured p99
  0.973 ms and maximum 1.234 ms against a 2.667 ms budget.
- Documentation content check: 24 pages, 132 records, zero errors/warnings.
  `./dev.sh --build`, changed-file rustfmt and `git diff --check` passed.
- Native UI checked on macOS 27.0.1 (26A434) in an isolated offline instance.
  Real tracks loaded through the UI and a native KEY drag set Platinum to -3.
  Both playing-key labels showed 10B; its library row retained 7B. Screenshot
  provenance and binary hash are in `assets/screenshots/capture.json` under
  `harmonic-sounding-key`. This instance had no audio device or stem cache.

The original live playback instance was not restarted. Offline PCM execution
and native rendering/control checks are established; subjective listening,
physical-device playback of this build and animation timing are not claimed.
Private source paths, library backup, plans, logs and rendered audio remain
ignored under `target/harmonic-review-20261017/`.
