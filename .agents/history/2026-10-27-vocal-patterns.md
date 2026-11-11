# Local repeated vocal evidence

The user identified a missing distinction: a rhythmic vocal sample need not
wait for a sung sentence to end. Brain Power is the development example.
The objective is to preserve words and musical foregrounds while making
repeated material available for independent stem fades. Fewer terminal cuts
is not an acceptance criterion.

## Implementation

- `mixless-protocol::vocal_pattern` requires recurrent vocal envelopes,
  spectral proportions and short-note sequences over at least four repeats.
  It checks the edit neighborhood separately, so a new word cannot inherit
  the surrounding loop's classification. Silence, missing evidence, held
  notes, regular syllable timing alone and changing local tempo do not grant
  permission. There is no track-name condition or whole-song chop label.
- The planner can fade a corroborated repeated part. Hard entries/exits
  still preserve each individual slice. Sung foregrounds, other instruments,
  grid, harmony, peak completeness and manual cues retain their checks.
- Local evidence is cached only while the planner borrows the two immutable
  analyses. Nested calls restore the previous scope; reanalysis and pointer
  reuse cannot inherit another input's result.
- Successful part fitting is ranked by its heard vocal collision rather
  than a collision between the original unmixed tracks. An unsuccessful fit
  restores the ordinary collision deduction.
- Sustained loss of the outgoing vocal/melodic foreground also contributes
  to the existing soft energy cost. Loud drums cannot hide that loss. The
  comparison follows the outgoing source's own natural continuation over a
  local bar; a muted incoming foreground does not invent an existing loss.
  This changes candidate preference, not phrase safety thresholds.
- `ORDER_VERSION=16`. No new persisted analysis field, database migration,
  source reanalysis or audio-callback work.

## Evidence and limits

Artifacts are under `target/automix-chop-20261027/`. The formal library was
backed up read-only. Its 76 source hashes, analysis version 26 payloads and
four playlist occurrence sequences matched the frozen v15 reference. Source
and executable hashes are retained. The baseline executable's SHA-256 is
`74fb6a419fd1f1d6849a7471cb92aa4ee1636415933e8c3aaee5955a2ab2e9eb`.

`final-patterns.json` samples local evidence every 0.5 seconds. Brain Power
has corroborated samples around 5.5–10.0 and 15.5–20.5 seconds (9.5 sampled
seconds, envelope correlation at least 0.953 and note correspondence at
least 0.80). This does not mean its remaining audio is a sung sentence.
Longer, changing or less clearly transcribed samples remain unknown.

The old short-note-density heuristic flagged at least one window in 73 of
76 recordings; the new detector flags local windows in four. These are
coverage measurements, **not** manually measured false-positive rates.
The three original sung-line complaints are not granted a repetition
exception in this sweep. The whole library is regression/development
material; there is no untouched listening holdout or human phrase annotation.

Current three-band vocal measurements and note transcription cannot prove
phonetic identity. In particular, closely repeated short lyrics may resemble
sampled material. Higher-resolution PCM comparison is retained separately in
`vocal-pcm-comparison.json`; it also does not establish lyric semantics.

## Rejected intermediate results

Replacing the old density test alone exposed a pre-routing ranking problem:
Don't trust me → Indigo Horizon changed from an eight-bar exchange to a
40-bar exchange with 9.15 seconds of cumulative foreground-gap flags and a
2.8-second longest run. This intermediate candidate was not accepted as an
improvement. Ranking the fitted routes reduced its longest flagged run to
0.35 seconds. The original eight-bar candidate remains feasible; replay is
recorded in `routed-score/electronic-replay.log`.

Further inspection separated losses from authored breaks. For example,
Don't you want me → Ember has a source-authored drums-only passage while B
is muted. A detector comparing against every *available* incoming stem can
flag this even when no heard foreground has been removed. Cumulative gap
seconds are therefore review flags, not a calibrated quality score.

Punching Holes is an explicit classification disagreement: the repeat
detector recognizes a short local motif, while the independent v5 observer
joins surrounding activity into an approximately 86.9–165.55-second vocal
phrase. A fade in the motif consequently produces a phrase-damage flag.
Keep this disagreement visible; neither algorithm is human ground truth.

Brain Power → Still in my heart remains constrained by the long protected
peak and the available next-track entries. Repetition recognition alone
does not resolve its terminal handoff or establish that its approximately
16 dB render-level jump is acceptable.

## Reproduction

The final candidate and all final comparisons are in `foreground-score/`;
the parent and `routed-score/` contain superseded experiments. Do not mix
their outputs. The final harness plans three cycles plus successor context
for fixed ORDER, fixed BEST and freshly computed BEST. Occurrence identities,
inherited entries, tempo and pitch are retained. The independent v5 observer
runs with 1/2/5-percent audibility and 1/2/3-pulse phrase-grouping sensitivity.

```sh
cargo test --locked -p mixless-protocol vocal_pattern
cargo test --locked -p mixless-mixplan --lib
cargo build --release --locked -p mixless-mixplan --features audit --example automix-audit
python3 target/automix-chop-20261027/foreground-score/run-plans.py
python3 target/automix-chop-20261027/foreground-score/fresh.py
python3 target/automix-chop-20261027/foreground-score/evaluate.py
```

The library also has an example named `automix-audit`; copy the mixplan
executable to an immutable artifact path before building the workspace.

The final code passes 671 workspace tests with two native HID discovery cases
excluded; both excluded cases pass separately in fresh processes. An earlier
unfiltered run failed the native discovery caller timeout. Desktop tests pass
233 cases (seven ignored). The dev bundle builds as 0.1.0-beta.5, build 1.
Changed-file rustfmt and `git diff --check` pass. Detector tests cover gain,
source offset, speed, half/double grids, held vowels, changing syllables, an
overlaid singer, missing evidence, tempo variation and a new central word.
Planner tests additionally cover slice-safe boundaries, cache lifetime, fade
windows and loud drums masking a removed foreground versus an authored break.

All 12 final queue comparisons completed: 684 evaluated transitions and 36
complete cycles. Eighteen real engine renders total 515.63 seconds, reproduce
the supplied plans at f32 precision and contain no nonfinite or clipped
samples. All complete playback. Punching Holes → Feel This Good changes its measured
A-stop RMS jump from +14.38 to -0.24 dB; its phrase-classification disagreement
still prevents treating this as listening acceptance. Brain Power's frozen
handoff remains +16.01 dB before and after. Some fresh-BEST proxy metrics
worsen, including aggregate foreground-gap flags; do not claim an overall
no-regression result. The report preserves those changes and unknown loops.

Numerical regression, real engine rendering and human listening are separate
evidence categories. Final numerical tables and check results are in the
[generated report](../../target/automix-chop-20261027/foreground-score/report.md).
No human listening or new native UI acceptance
is claimed for this algorithm-only change. No commit, push, installation or
release was performed.
