# Independent stem release and DJ technique experiments

The user asked whether other DJ techniques, especially better stem swaps, could
improve the remaining doubtful terminal handoffs. This continues the previous
whole-library strategy search. No commit, push, installation or publication was
requested. Existing uncommitted work was preserved.

## Evidence

- Evidence root: `target/automix-stem-relay-20261026`.
- SQLite backup of the current library, read-only source verification: all 76
  analysis payloads match the reference exports, all are analysis version 26,
  and all 76 current audio BLAKE3 hashes match. No formal-library mutation.
- Baseline: v13 from `target/automix-balanced-20261026`, four playlists (Melodic,
  Electronic, MUG and DnB). These are regression recordings, not holdouts.
- Final audit binary SHA-256:
  `ddda93b930557929a32bf39f6e2e47299c3b2c9935d64421667c44ba165ef792`.
  `initial-source`, `final-source`, `initial.diff`, and source verification JSON
  preserve provenance. Subsequent formatting verification found no source delta.

## Retained change

`melody_release` changes the contour of an already selected outgoing instrument
fade. Its target amplitude is the square root of its previous normalized
amplitude, bounded by the original full-level reference. This retains more of
the synth/pad tail without extending its source window or release point.

Eligibility requires at least four bars, BeatBlend, prepared stems, a monotonic
outgoing melody release, and closed incoming vocal/melody/bass envelopes during
that release. It does not infer harmonic permission from global key labels or
from quiet source material. Manual IN/OUT, missing stems, native-clock bridges,
loops, scratch and FX processing are excluded. Final foreground preservation
still checks the candidate before accepting it.

The implementation leaves the crossfader and all incoming controls exactly
unchanged. Where necessary it raises only A's channel within the existing 6 dB
limit and inversely scales A's other three stem controls. It never raises the
melody above its original full-level amplitude. Source clocks, cues, rate, pitch,
summary score and inherited state remain unchanged. ORDER_VERSION is now 14.
No callback, analysis-schema, database or protocol change was added.

## Alternatives examined

1. Unconditional slower melodic release reduced gaps but increased local tonal
   conflict and rhythmic exposure. It was rejected; `tail-comparison.json`
   preserves those increases. The conditional implementation was motivated by
   separating the tonal foregrounds, not by changing observer thresholds.
2. A bounded 4/8/16-bar percussion-preparation prototype kept the old handoff
   endpoints and carried rate/pitch, then asked the existing full part fitter
   for a legal independent exchange. The prototype produced no accepted fit on
   the fixed current Best chains. Orbit → Feel This Good and Feel This Good →
   Underwater have matching endpoint beats, but the local whole-window key
   observations do not establish safe simultaneous tonal foregrounds. The
   current fitter cannot turn these into a complete legal arrangement merely
   by starting B earlier. This is **not proof that drum-only preparation is
   impossible**: whole-window key evidence is not the same as the eventual
   routed parts. Prototype source, test binary identity and raw output are
   retained in `prelude-prototype.rs`, `prelude-binary.json`, `prelude-audit.log`.
   No prelude code remains in production.
3. A short incoming drum loop followed by a separate foreground exchange remains
   a useful candidate when the available pre-roll is too short. It needs
   explicit loop-release/source-clock verification and loop-aware independent
   evaluation; the current observer correctly reports loops as unavailable.
   This round did not implement or claim validation of that strategy.
4. A stem-specific echo after a complete vocal/melodic release could carry a
   natural tail. It cannot justify cutting an unfinished line. Current deck FX
   sends do not by themselves provide independent per-stem FX routing, so no
   automatic echo workaround was enabled.

These ideas are informed by the official descriptions of independent stem
control and stem-specific echo, not third-party DJ code:
[Native Instruments stem mixing](https://www.native-instruments.com/blogs/blog/how-to-mix-stems),
[Serato Stems](https://support.serato.com/hc/en-us/articles/5700921209615-Using-Stems),
[Native Instruments drum pattern bridges](https://www.native-instruments.com/blogs/blog/pattern-player-traktor-pro).
Their applicability to the current recordings is an engineering hypothesis,
separate from what those product documents establish.

## Comparison

All reports use the same observer v4 and unchanged signal thresholds, including
1/2/3-pulse vocal grouping. The comparison tool now also requests review for
increased signal-threshold or unknown exposure, and compares individual queue
occurrences when adjacencies match. A smaller total gap cannot hide another
pair's new conflict or gap. Such alerts request review, not a listening verdict.

| Queue | Pairs | Bed gap before | Bed gap after | Terminal before / after |
| --- | ---: | ---: | ---: | ---: |
| Fixed ORDER | 80 | 26.20 s | 25.25 s | 35 / 35 |
| Frozen old Best | 80 | 53.95 s | 51.25 s | 25 / 25 |
| Current Best, frozen | 80 | 66.60 s | 63.15 s | 13 / 13 |
| Freshly recomputed Best | 80 | 66.60 s | 63.15 s | 13 / 13 |
| Reported DnB chain | 15 | 40.55 s | 38.95 s | 5 / 5 |

All four freshly recomputed Best orders match v13. Every playlist separately has
non-increasing bed gap and no new observer review flags. Default vocal damage
remains zero on the observed material; fresh Best observes 77/80 pairs, with
three unavailable. Three-pulse grouping still flags 13 fresh-Best pairs, unchanged
from baseline; this uncertainty must not be hidden behind the default result.

Across the 255 fixed-chain plans, 20 changed. Every other plan field, source
clock, outgoing endpoint and incoming control is exactly unchanged. Dense
node/midpoint checks found maximum amplitude difference below 2.7e-8 in A's other
three parts. `state-invariants.json` records each occurrence.

Reported chain examples:

- Lost & Found → Feel This Good: gap 4.85 → 4.40 s, same A exit 132.47107 s.
- We Are The Energy → In The Echo: gap 6.35 → 5.90 s, same A exit 171.0668 s.
- In The Echo → Underwater: gap stays zero, plan unchanged, exit 84.17046 s.
- Current Best's closing Orbit → Feel This Good and Feel This Good → Underwater
  remain terminal handoffs. This change does not claim to solve those exits.

## Validation and limits

- Workspace: 661 passed, 28 ignored. Desktop: 233 passed, 7 ignored.
- Observer/comparison: 38 passed. New planner tests cover other-part invariance,
  incoming state, multiple gains and time scales, quiet incoming tonal leakage,
  intentional re-entry, bass-only material, manual cues, missing stems,
  continuously active voice and non-blend/short plans.
- Six before/after pairs rendered from real cached stems: 12 WAVs, 439.43 s,
  exact supplied plans checked at f32 precision, no nonfinite samples or clipping.
- Continuous Melodic set: 1493.43 s, source tempos 135–184 BPM, 12 completed
  handoffs with carried-state checks, peak 0.601127, no nonfinite samples or
  clipping. Full PCM was independently scanned, not just renderer logs.
- Development bundle rebuilt at `target/app/Mixless.app`; changed Rust files
  pass edition-2021 rustfmt with child traversal disabled, and `git diff --check`
  passes. Recursive format checks encounter pre-existing mixed style in other
  planner modules; those unrelated files were left intact.
- No subjective listening acceptance or new native UI/live-device acceptance
  is claimed. The renders prove execution, not musical preference. Remaining
  multi-second bed gaps and terminal handoffs are still explicit in the report.
