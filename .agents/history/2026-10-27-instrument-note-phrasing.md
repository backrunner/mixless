# Instrumental note groups and exchange timing

Historical experiment. The subsequent [voice-leading investigation](2026-10-27-instrument-voice-leading.md)
supersedes the withdrawal below: it identifies a cycle-scoring defect and
measurement false positives, restores note candidates and adds polyphonic
pitch/time tracking. The observations below remain evidence of that earlier run.

The earlier observer merged overlapping sustained pitches into one interval and
could describe that interval as if it were a melodic sentence. It is not. In
the previously rejected Brain Power → Still example, Still's notes at MIDI 71,
52 and 36 overlap to form a 0.093–5.431 s union, but the new group at the 3.104 s
entry has its own attack. The old 1.4 lost-note-seconds observation alone does
not establish a clipped instrumental phrase.

## Shared evidence and planner behavior

`mixless_protocol::instrument_boundaries` derives evidence from existing stem
notes and frames. No stored analysis fields, database migration, model version
or audio callback change is involved. ORDER_VERSION remains 17 because the
experimental planner changes were withdrawn.

- Simultaneous attacks stay polyphonic groups; their storage order is not a
  melody. Only nearby fragments of the same pitch without a new measured attack
  are joined. Different overlapping pitches never become one held note.
- Repeated motifs compare one-to-one pitches, onset positions, durations and
  intervals between matched pitch-centroid groups over three complete cycles.
  A supported downbeat selects the phase; recurrence alone would be equally
  true at every cyclic rotation. Note and contour agreement must both reach
  85%. At least three attack groups and actual pitch movement are required.
- Releases and subsequent attacks remain separate. A rest needs at least half
  a normalized local pulse and a 6 dB measured instrumental-power retreat.
  Missing transcription without that acoustic support is not a rest.
- A held crossing voice, single-pitch tremolo, stable pad, low-confidence notes,
  weak clock or missing frames add no motif-ending permission. Their absence
  of positive evidence is not a blanket prohibition on an otherwise safe fade.

The retained implementation is diagnostic. Neither rests nor motifs grant new
fade permissions. The production planner, order scoring, vocal protection,
manual cues and peak handling keep their pre-task behavior. Optional memoization
is tied to immutable borrowed inputs; it is not installed on the playback path.

The first experiment mixed extra windows into an existing four-choice cap.
The audit exposed candidate crowding: a new proposal could evict a viable
structural alternative before comparison. Separate candidate budgets removed
that defect, but a second full audit still made fresh DnB BEST worse: over three
cycles, foreground-gap time grew 59.25 → 62.85 s, abrupt tonal-loss flags grew
3 → 6 and measured harmonic-conflict exposure increased, while tail-short count
stayed at six. The version is retained in `motif-candidate/` and was rejected.

A rest-only ablation (`short-rest-candidate/`) reproduced the same adverse queue
and observations. It would therefore be incorrect to attribute the regression
specifically to motif permission. A further ablation restored the original
0.75-bar minimum audible fade duration (`duration-check/`); it still produced
the same reordered DnB queue. The duration bypass was a real implementation
defect, but repairing it did not resolve this regression. All new note-derived
fade permissions were withdrawn; no threshold was tuned to force the desired
playlist result. A regression now asserts that diagnostic note rests do not
expand the production arrangement search. Final acceptance evidence lives in
`target/automix-instrument-phrasing-20261027/final/`.

## Independent evaluation and limits

Observer v6 keeps per-note attack/release observations and control speed, with
explicit unknown coverage. It measures the heard tonal lane, not isolated MIDI
note energy. A layer must reach 1% of the heard mix in at least one sampled
position; very faint numerical fade tails do not become damaged heard notes.
Constant trim is normalized when comparing the note's control change. The old
merged sustained-note count remains a descriptor, not a phrase-damage gate.
Both baseline and candidate are re-evaluated with the same final observer.

The available corpus is 76 previously inspected recordings in four playlists;
it is development/regression material, not an independent holdout. Current
source hashes, analysis payloads (v26), stem version (v2) and playlist occurrences
were verified using a consistent read-only backup. The detector supplies 140
acoustically supported rests and three repeating-cycle boundaries across 56
recordings. These counts measure available evidence, not semantic accuracy.
The motif examples occur in Loving, Loathing and Feel This Good. Raising
the stricter agreement threshold from 85% to 86% removes the former example;
the other two exceed 97% note agreement. This sensitivity is not an accuracy
estimate and is another reason not to use recurrence alone as a fade permission. Brain Power's
internal relative kit retreat still lacks positive instrumental-cycle evidence.

Existing transcription is performed on the combined instrument signal,
including bass. Pitch centroids do not isolate a lead melody from accompaniment.
The implementation does not claim to identify every musical sentence, cadence,
pad or reverberation tail. The thresholds are mechanism-based proposals,
not listening-calibrated probabilities. See Spotify's
[Basic Pitch documentation](https://github.com/spotify/basic-pitch) for the
single-instrument input limitation; related grouping research examines pitch,
onset intervals and rests jointly in the
[ISMIR 2008 proceedings](https://archives.ismir.net/ismir2008/2008_ISMIR_Proceedings.pdf).

Counterexamples and transformations cover evolving contours, repeated single
notes, chords, sustained overlapping voices, transcription holes, missing
frames, weak clock, multiple tempo-analysis segments, tempo changes, gain,
transposition, time shift, speed and equivalent half/double pulse readings.
The final artifact report records inherited-state plans, fixed/fresh queues,
independent observation, real stem renders and build results separately.
None of those establishes listening acceptance. No formal-library write,
installation, native playback interruption, commit, push or release is included.

## Remaining work

These inputs support finer observations, not reliable lead-phrase completion.
The next step is to separate bass from the remaining instrument transcription,
track simultaneous voices over time and distinguish a foreground melodic line
from accompaniment. Candidate fades then need scoring on their actual heard
attack, sustain and release, with full inherited-state queue regression. This
turn does not claim that Brain Power has acquired a safe new internal exit or
that the existing conservative planner is now musically optimal.

## Final validation

The final audit re-planned 684 measured transitions (four playlists, three
cycles each of ORDER, frozen BEST and recomputed BEST), plus successor context.
Every plan and fresh queue matches the pre-task v17 baseline. Observer-v6
measurements are reused only after exact plan/input identity is verified.
Six existing renders from this turn contain the same plans selected by the
final run; their 68 seconds of PCM have zero nonfinite or clipped samples.
They are reused render evidence, not new listening evidence.

Final checks: 686 workspace tests passed with two independently failing native
HID discovery tests excluded; 233 desktop tests and 60 observer tests passed.
Both HID tests failed with channel timeouts in separate fresh processes.
A CPU-concurrent workspace pass hit the existing fake-download timeout; the
complete remaining workspace passed serially after planning completed.
The development bundle built successfully, changed Rust formatting passed, and
whole-workspace formatting retains unrelated pre-existing differences.
Detailed logs, hashes, tables and boundaries are in
`target/automix-instrument-phrasing-20261027/final/report.md`.
