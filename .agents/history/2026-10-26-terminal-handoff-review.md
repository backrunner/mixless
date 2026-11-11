# Terminal handoff case review

The user asked whether the remaining played-out short handoffs are actually
justified. This is a read-only assessment of ORDER_VERSION 13 / analysis 26,
not a new planner fix or a listening acceptance claim.

Evidence: `target/automix-terminal-review-20261026/report.md` and its linked
per-case JSON, candidate enumeration, counterfactuals, and WAV files. These
artifacts contain private library material and remain ignored.

## Scope

- All 73 terminal occurrences across the four saved playlists: 13 fresh BEST,
  35 fixed ORDER, and 25 frozen old BEST. There are 69 distinct directed pairs;
  loop edges and their different carried states remain separate occurrences.
- Kept actual inherited entry, earliest scheduling time, rate, and pitch.
  Enumerated current phrase points and duration candidates with shift/native/
  held variants and no outer score pruning: 737,103 candidate calls, 386
  deduplicated feasible plans, 161 independently measured alternatives.
- This is bounded by the current point generation and arrangement search.
  Finding no candidate does not establish that an earlier musical transition
  is impossible. An earlier candidate is not necessarily a better one.

## Findings

- Ghost Voices → Reborn need not wait until 258.19 s. A feasible 146.19 s exit
  preserves the same incoming entry, ending rate, and pitch. Replacing only
  this plan in the 30-handoff MUG chain leaves all subsequent pair metrics
  unchanged, with no carried-state violations. Bed-gap exposure falls from
  14.55 to 13.85 s, played-out count from 8 to 7, and three-pulse vocal flags
  from 6 to 5. Default vocal flags remain zero; coverage stays 29/30. This
  counterfactual has not been selected by the production planner.
- Call My Name → Your voice so... has 3.74 audible bars. Its earlier exits
  trigger tonal-loss flags, while a 4.10-bar alternative has greater local
  roughness. Crossing the four-bar reporting threshold is not itself a win.
- Don't you want me → Ember retains 3.90 s of cumulative bed-gap exposure.
  Earlier alternatives also have substantial gaps, so choosing an earlier
  exit alone would not fix its arrangement.
- 漂流 → Used to Be and Angelic Sphere → Qualia have source-exit reasons
  under their inherited entrances, but their abrupt tonal-loss flags leave
  the hard cuts unaccepted. Angelic Sphere has a same-exit overlap alternative
  with a harmonic tradeoff; it is not a proven replacement.
- Orbit's late exit has continuous-vocal evidence. Its Feel This Good entry
  at 77.48 s misses the first peak's beginning, which contributes to the next
  transition waiting until 295.41 s. From the initial source entrance, the
  same Feel This Good → Underwater pair exits at 200.24 s. Judge this chain
  with inherited state, not by pair title alone.
- Removing only the extension of an owed peak to the full contiguous
  Drop/Chorus run changes seven cases' obligations, but adds no earlier
  feasible candidate cases across all 73 occurrences. This does not justify
  globally relaxing the rule or treating every rejected alternative as
  musically unsafe.

## Evidence boundaries

Fourteen occurrences have at least one feasible exit four seconds earlier
(fresh BEST 4, ORDER 5, frozen BEST 5). The fresh BEST terminal subset has two
cases with more than one second of cumulative bed-gap exposure, two with
abrupt tonal loss, and two flagged by three-pulse vocal grouping. These are
signal diagnostics, not lyric annotations or subjective quality scores.

Rendered all 13 current BEST terminal transitions and two alternatives with
real stem PCM and carried offsets: 15 clips, 329.51 s, zero non-finite or
clipped samples. Rendered plans match supplied plans at f32 precision. No
human listening or native playback acceptance was performed.

Temporary diagnostic Rust changes were archived, then restored byte-for-byte.
The original diagnostic binary was rebuilt and its SHA-256 reproduced. The
production source fingerprints still match the preceding v13 validation.
No application rebuild, install, commit, push, or release was performed for
this assessment.
