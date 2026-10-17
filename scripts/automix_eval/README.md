# AutoMix evaluation, observer version 1

This observer is independent of the planner's candidate scores. It reads exported
`TrackAnalysis` and `MixPlan` JSON, uses one reference analysis for every variant,
and reports separate diagnostics instead of a weighted quality score. Python 3.10+
and its standard library are sufficient. No audio or personal library is committed.

## Frozen hypotheses and measurements

These definitions were written before evaluating the candidate with this observer.
The reported examples motivate mechanisms, not track-specific exceptions.

| Concern | Measurement | Interpretation / counterexample |
| --- | --- | --- |
| Silent preparation counted as mixing | Seconds both decks exceed 2% of their active reference power; audible master bars | A 30-bar preparation followed by a cut is not a 30-bar blend |
| Off-beat drums or rhythmic instruments | Local reference-grid period error after playback rates, and folded phase error, during audible drum or onset-bearing instrument overlap | Half/double representations are equivalent; low-confidence grids are unknown, not aligned |
| Matched beats but an abrupt change in drum activity | Absolute difference of heard drum-onset activity between adjacent one-second windows, including cuts | Aligned grids do not imply similar rhythmic density; natural fills can also produce this diagnostic |
| Clashing vocals/chords | Pitch-shifted chroma interval roughness weighted by the quieter heard tonal foreground | A compatible key label does not prove local note compatibility; intentional dissonance can be valid |
| Fading an unfinished voice | Seconds of active/held outgoing voice attenuated more than 6 dB while still intended to be heard, plus notes crossing the final audible exit or first audible entry | A vocal stem intentionally absent for the whole transition is not a truncated phrase; quiet held notes count |
| Peak energy collapsing | 1-second dry-power deficit against the louder available unmixed source, conditional on outgoing energy remaining above half its active reference | Natural release into a quiet arrangement is separate from attenuating an ongoing peak |
| Crowded foreground | Seconds both post-control tonal mid/high bands exceed 10% reference power | This is competition exposure, not a verdict that all layering sounds bad |
| Sudden chord entrance | Largest 250 ms positive control-induced tonal amplitude rise, weighted by source power | Natural source attacks are not control jumps; boundary cuts must be measured too |
| Set-wide pitch drift | Absolute semitone-seconds through transitions, intervening solo playback and final track; maximum offset; audible vocal exposure during pitch motion | Native restoration can reduce burden but must not bend a held vocal |
| Waiting for a second drop / playing out | Legacy cuts, fallback and terminal counts; delay after the first complete observed drop | Structure labels are diagnostics, not independent musical truth or proof of a feasible earlier exit |
| Missed percussion withdrawal cues | Explicit annotated-event recall/offset, when annotations are supplied | Detector output cannot serve as its own ground truth; missing annotations remain unavailable |

Sampling uses 50 ms wall time, not a fixed fraction of the planner's bars. Source
clocks, rate, pitch, equal-power crossfader, level, three EQ bands, logarithmic
filter interpolation and four stem lanes are combined. The analysis exposes only
three stem groups; instrument low-band power is the bass proxy, matching the
meter contract (lane order vocals, drums, instruments, bass). Each source is
normalized to its 75th percentile active band power. This is a controlled
equal-active-level comparison, not a reconstruction of hardware trim or LUFS.

The power observer is an incoherent three-band approximation at 70/600/5000 Hz.
It does not model phase cancellation, limiter, separator leakage, resonance or
wet FX tails. Loop/scratch/insert processing is excluded and reported explicitly;
FX send plans retain dry estimates with a warning. Grid phase is conditional on
the reference grid and its confidence, not independent PCM beat accuracy.
Drum-only grid diagnostics are reported separately from broader rhythmic
foreground diagnostics. The latter include instruments with measured onsets in
the surrounding second and can flag pads with weak attacks; inspect those cases
before attributing a timing fault to the planner. Sub-millisecond agreement with
the same grid used for planning is numerical consistency, not PCM precision.
Chroma is reconstructed separately from vocal and instrument notes, then weighted
by their heard powers. Muted vocal notes cannot supply evidence for instruments.
At least 80% of heard tonal power must have note evidence; otherwise harmony is
unknown. Instrument/bass note separation and octave ambiguity remain unresolved.
The grid-confidence floor is 0.15. Active voice uses activity >= 0.35 or a held
note of confidence >= 0.5; an exposed vocal phrase needs 0.2% reference power at
some point. These are observer assumptions, not tuning changes to the planner.
Quantitative improvements do not establish listening quality.

Drum-onset activity is the source's normalized drum onset feature multiplied by
the square root of its heard normalized drum power. Adjacent one-second windows
are compared at each 50 ms step, using 0.1/0.25/0.5 diagnostic thresholds. This
combines rhythm activity with level/EQ/stem masking, without treating a change of
rhythm as inherently wrong. It is a flux-activity proxy, not a count of drum hits.

Every metric includes eligible/observed exposure or an explicit unavailable
result. No overlap is not a perfect rhythm/harmony score. Report per-set values,
macro averages over sets (keep Downloads separate), worst pairs and coverage.
Also report 1/2/5% audibility, 20/30/50 ms phase tolerance, 3/6/9 dB voice/energy
threshold sensitivity without selecting the threshold that flatters a variant.

## Reproduce

Build `automix-audit` with `--features trace` at both revisions, using separate
Cargo target directories, and retain each binary with its SHA-256. Cargo's shared
example output path can otherwise still name the other checkout's executable.
Use SQLite's backup API to obtain consistent copies. Reanalyze only the new copy.
Then generate all fixed/Best comparisons (the output directory must be new):

```
python3 scripts/plan-automix-matrix.py --baseline /path/to/old-audit --candidate /path/to/new-audit --old-db old.db --new-db current.db --baseline-revision OLD --candidate-revision NEW --output evidence
```

This harness refuses snapshots containing manual cues rather than ignoring them.
Source content hashes must be verified against the actual audio separately; the
matrix generator checks agreement between snapshots, not current file contents.

```
python3 scripts/evaluate-automix.py --analyses reference.json --plans plans.json --output report.json
python3 scripts/evaluate-automix.py --manifest evidence/matrix.json --output comparison.json --sensitivity
python3 -m unittest discover -s scripts/automix_eval -p 'test_*.py'
```

A manifest contains `reference_analyses` and `runs`, each with `name`, `set`,
`variant`, `order` and `plans`. Paths are relative to the manifest. Optional
`annotations` contains a list of `{track_id, time_sec, tolerance_sec}` and
`cues` maps track IDs to automatic cue times. Human annotations must be recorded
before comparing variants. Input SHA-256 hashes and observer settings are saved.

Compare fixed orders first, then freshly planned Best orders separately: changing
the pairs changes the task. Compare the pre-series baseline on old analyses, the
baseline on current analyses, and current planner on current analyses to separate
analysis and planner effects. Previously inspected recordings and alternate
encodings belong to the regression group. None of the existing sets constitutes
an untouched holdout. Review synthetic negative cases and gain/time/tempo/grid
representation transformations before interpreting real-set results. Do not tune
production thresholds against this report alone.
