# AutoMix evaluation, observer version 8

This observer is independent of the planner's candidate scores. It reads exported
`TrackAnalysis` and `MixPlan` JSON, uses one reference analysis for every variant,
and reports separate diagnostics instead of a weighted quality score. Python 3.10+
and its standard library are sufficient. No audio or personal library is committed.

## Measurement definitions

These definitions describe the current observer. Thresholds are engineering
assumptions, not listening-calibrated probabilities or preregistered accuracy
claims. The reported examples motivate mechanisms, not track-specific exceptions.

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
FX sends are also unavailable because a dry estimate cannot establish heard continuity. Grid phase is conditional on
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

## Measurement contract

The uncalibrated `integrity.score` is retired (`null`). The only score is the
transition score in [DJ practice comparison](#dj-practice-comparison), whose point
values are stated and uncalibrated. `instrument_bed_gap_seconds` remains an arrangement descriptor;
`voice_carried_bed_gap_seconds` reports the part carried by an audible voice.
`foreground_gap_seconds` screens for removal of the outgoing vocal/instrument
foreground relative to its natural continuation. A louder, still-muted incoming
track does not establish a lost foreground. Version 7 corrects that false positive;
both sides of a comparison must be recomputed. This screen covers the scheduled
overlap only: a zero-bar cut has no overlap exposure, so a smaller value alone
does not establish better arrival. Compare equal post-exit context and render
the actual arrival when choosing between a cut and a longer arrangement.
The same reference correction applies to `peak_deficit_db`: compare heard power
with the outgoing track's natural power, not a louder silent incoming peak.
Incoming note/voice damage and foreground competition remain separate screens.
Drums alone cannot hide this risk. An intentional percussion bridge may still
trigger it and needs review. Vocal phrase loss remains separate from instrumental
control observations, even when another voice carries the foreground.

Exposed vocal episodes bridge rests of two normalized pulses (also test one and
three). Inspect the whole episode after muting or before revealing it. Report
at least 150 ms of exposure with more than 10% control-induced energy loss and
150 ms below half gain. Sustained instrument notes use the same exposure/loss
diagnostics, on confidence >= 0.5 notes held at least one normalized pulse.
These legacy descriptors are not sentence detection. Overlapping held notes are merged;
sequential notes are not automatically a complete melodic phrase. Low-band
instrument power is only an approximate bass contribution. Human annotations
are still required for lyric and melodic phrase completeness.

Observer v6 also reports `instrument_note_events`. It retains each note's pitch,
attack and release instead of treating overlapping pitches as one sentence.
Five samples across each note describe a hidden attack, attenuated release and
maximum tonal-lane power-gain change per reference pulse, relative to that note's
maximum sampled gain (constant level trim cannot manufacture a damaged note).
This diagnostic pulse is normalized from the recording's global BPM to 80–160
BPM; it does not follow local tempo changes. It is not a local beat-accuracy
measurement.
The analysis does not
isolate each note's energy or reliably distinguish lead, accompaniment and
reverberation. These are control observations for review, not phrase-damage
labels or a listening score. Entirely unplayed notes do not count as damaged
heard notes. Missing local evidence is counted separately. The legacy merged
`sustained_instrument_risk_pairs` is retained as a descriptor for old audits;
it no longer raises a phrase-damage regression alert on its own. Comparisons
require both reports to use v6 and inspect individual changed note observations.

Source-to-wall inversion uses the same source clock as sampling, including
incoming launch, rate ramps, short explicit curves and solo recovery. Exposure
is measured in wall seconds. Outgoing phrase context inside the previous
transition uses that transition's actual incoming controls. Remaining solo
context assumes unity lanes, retaining modeled rate and pitch; it does not
reconstruct live user fader edits. Non-monotone source curves, nonpositive rates,
loops, scratches, inserts and sends remain unknown.

`handoff` separates:

- Nominal file time remaining, last observed outgoing sound and tonal foreground,
  first observed incoming sound, and active source content remaining.
- The raw source's last above-threshold frame from container EOF. Digital silent
  padding changes file duration but not the observed active endpoint. This is
  not a semantic Outro detector; quiet musical tails require sensitivity review.
- Accumulated overlap, individual runs, longest uninterrupted run, and a separate
  gap-tolerant span. The latter bridges observed rests up to half a normalized
  local pulse, capped at 250 ms; unknown gaps are never bridged. Weak grids make
  this descriptor unavailable. The span includes bridged rests; accumulated
  overlap does not. Neither is a musical acceptance gate.
- Lower/upper overlap bounds and explicit eligible/observed/unknown seconds.
  `audible_overlap_seconds` and `audible_overlap_bars` at pair level are lower
  bounds, not known zero when evidence is unavailable.
  Bounds describe the scheduled overlap window; unmodeled wet tails outside
  that window are not bounded by the observer.
- `legacy_tail_short`, the old 8%-or-8-second / under-four-clock-bars screen.
  It returns `null` for an unobserved terminal overlap or unsupported DSP. A
  known early nominal exit can be false even if overlap is unknown. This field
  and the four-bar descriptor intentionally remain clock-alias-sensitive and
  must not serve as regression gates. Physical seconds are reported alongside.
- `heard_tail_position`, applying the legacy tail window to the estimated heard
  endpoint and observed active source span. It says nothing about transition
  quality. If no outgoing sound is observed in the one-second pre-context or
  transition window, the endpoint remains unknown rather than becoming `t_out`.

Every threshold is a signal-observer assumption. One/two/five percent mean
relative power, not a hearing threshold or fader percentage. `--sensitivity`
retains all three settings. Source-contained drop counts are renamed
`source_contained_drop_count`: they describe positions, never certify that a
complete drop was heard. Existing recordings are regression material, not an
untouched listening holdout. Rendering and numeric correctness are separate
from listening acceptance.

Missing observations report coverage, not a good score. Foreground diagnostics
with frame holes are partial. Unsupported plans contribute unknown eligible
exposure to each signal metric and have unavailable continuity diagnostics.
Reports from older observer versions must be recomputed; cross-version quality
comparisons are refused. Frozen-plan schema audits must explicitly describe
measurement changes instead of claiming planner improvements.

Version 2 corrects maximum absolute pitch accounting: include audible opening
solo playback and exclude offsets carried through empty or inaudible solo spans.
The semitone-second burden and transition measurements are unchanged.

Version 3 corrects source clocks when a plan omits its explicit source curve:
integrate rate against the device clock from the actual source origin and incoming
launch bar. Extend short curves through remaining recovery time. The outgoing
stop uses the scheduled overlap boundary, including zero-bar cuts, rather than
the end of a possibly longer source curve. Previously, an incoming cut could be
measured against its silent intro at source zero and falsely report extreme
energy loss. Recompute older reports before comparing their measurements;
version-2 snapshots remain historical evidence. These corrections do not change
the production planner or audio renderer. Loop processing remains unsupported.

Version 8 reads `incoming_varispeed`. An incoming deck playing with key lock
off sounds transposed by its speed, so its pitch is the pitch lane plus
`12 * log2(rate)` in the chroma, pitch-burden and pitch-motion measurements.
Plans without the field measure exactly as in version 7.

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

## DJ practice comparison

Each run also reports `realism` (version 3), computed from the measurements above
and the plan's own rate and pitch lanes. It never reads planner scores.

### Checks

| Check | Deviates when (strict / default / lenient) |
| --- | --- |
| `structure` | Any plan violation |
| `tempo` | Heard rate change beyond what the pair requires exceeds 5 / 10 / 20% |
| `key` | Heard transposition exceeds 0 / 1 / 2 semitones, one more when native keys clash |
| `settle` | The incoming track ends the plan more than 2 / 5 / 10% off the tempo the next pair makes useful, or transposed by more than 0 / 1 / 2 semitones |
| `exit` | The outgoing track plays out to its tail with under 6 s of heard overlap; lenient accepts it |
| `beat_sync` | More than 1 s of rhythmic overlap is over 20 / 30 / 50 ms off the grid |
| `harmony` | More than 2 s of tonal overlap has roughness over 0.25 / 0.4 / 0.55 |
| `vocals` | Over 0.5 s of exposed voice is attenuated by 3 / 6 / 9 dB, or a vocal phrase loses more than 0 / 0 / 25% of its energy; strict also rejects a note crossing the audible entry or exit |
| `energy` | Over 1 s of 3 / 6 / 9 dB peak deficit, a foreground gap over 0.5 / 1 / 2 s, or abrupt tonal loss inside an overlap; strict also counts the instant replacement at a cut |

A playlist can hold any tracks in any order, so the planner is not charged for
what the queue demands:

- Tempo. `forced_tempo_adjustment` is the smallest per-deck change that
  beatmatches the two native tempos when both decks meet in the middle, with half
  and double time folded. Only the change beyond it (`tempo_excess`) is judged.
- Key. When the native Camelot keys are neither equal, relative nor adjacent, one
  extra semitone is allowed at every level. An unknown key claims no allowance.
  Labels under 0.55 confidence still count and are reported as
  `weak_key_label_pairs`, because a wrong label can grant the allowance.

Tempo and key are charged to the transition that introduces them: the incoming
deck answers for what is heard and for the state it keeps, the outgoing deck only
when its carried state moves.

The queue allowance ends with the overlap. `settle` reads the state the plan
leaves, solo restoration included (`incoming_offset_end`). A kept tempo offset
is useful only towards the point where this track and the next meet in the
middle; the rest is `kept_tempo_excess`. A kept transposition has no allowance.
A track that returns to its own tempo and key after the handoff passes `settle`
even though `tempo` and `key` record what the overlap needed.

`exit` separates a cut from a play-out. A cut in the middle of a record is a
transition; a record that runs to the last 8% of its sounding length (at least
8 s) and is replaced almost unmixed is a safe handoff, not a mix. The nominal
exit stands in when no outgoing sound was observed at the cut.

### Score

Each check earns 1 point when it passes strict, 0.8 at default, 0.5 at lenient
and 0 otherwise. A transition's `score` is

`100 * (0.6 * mean check points + 0.4 * clean)`

where `clean` is 1 with no deviation at default, 0.5 with none at lenient and 0
otherwise. A clean transition therefore scores at least 88, one that is clean
only at lenient at most 76.7, and one with a check failing at every level at
most 53.3. A flawed transition still keeps the points of the checks it passes.
The run `score` is the mean over scored transitions.

A transition whose missing evidence could still hide a deviation is unscored. It
is excluded from the mean and bounds it through `score_bounds` (0 and 100 for
each unscored transition). A deviation known at every level is scored on the
observed checks. `clean_rate` and `clean_rate_bounds` report the same counts per
level without points.

The point values, the 0.4 clean share and every limit are engineering
assumptions. Report the three levels and the per-check counts beside the score.
The score treats a clean mid-record cut and a clean blend alike; read `overlap`
(heard bars, cuts, play-outs) with it.

### Published DJ shares

`dj_reference` compares each played track with shares measured in 1,557 real DJ
mixes (24,202 played tracks) by Kim et al., "A Computational Analysis of
Real-World DJ Mixes using Mix-To-Track Subsequence Alignment", ISMIR 2020
([arXiv:2008.10267](https://arxiv.org/abs/2008.10267), sections 6.1 and 6.2):

| Published share of played tracks | Value |
| --- | ---: |
| Tempo adjusted by less than 5% | 86.1% |
| Tempo adjusted by less than 10% | 94.5% |
| Tempo adjusted by less than 20% | 98.6% |
| Transposed | 2.5% |
| Transposed tracks moved by one semitone | 94.3% |

A track's tempo adjustment is the heard-time-weighted mean of `|rate - 1|` over
its entrance, solo playback and exit; transposition is the largest heard offset.
`chance_under_reference` is the binomial probability of at least the observed
number of outlying tracks if the run followed the published share.
`mean_gap_percentage_points` averages the absolute difference of the four shares.

These shares use no queue allowance and do not enter the score. That population
is mostly House and Trance, and its DJs chose compatible tracks, so a gap on an
arbitrary playlist describes the queue as much as the planner. `queue` counts the
pairs with a forced tempo change of at least 5% and with clashing native keys.
The paper shows transition lengths peaking at every 32 beats without publishing
shares, so `overlap` (heard and nominal bars, blends of at least four heard bars)
stays a descriptor.

`compare-automix.py` reports `realism` before and after. It raises
`realism_score` when the run score drops and `realism_deviating_transitions`
when any level gains a deviating transition. None of this is listening
acceptance, and no reference DJ mix of the evaluated playlists exists yet.
Recorded human transitions for the same tracks would be the direct comparison.

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

Compare all playlists with one review report, keeping fixed queues and fresh Best
as separate named comparisons:

```
python3 scripts/compare-automix.py --manifest comparisons.json --output review.json
```

`comparisons.json` contains a `comparisons` array. Each entry supplies `name`,
`analyses`, `baseline` and `candidate`; paths are relative to the manifest and the
last two refer to observer reports produced with `--sensitivity`. Optional
`baseline_run` and `candidate_run` select a named run in multi-run reports.

The comparison checks reference-analysis hashes, observer version/settings,
track occurrence counts and explicit queue/cycle coverage when supplied. Both
incoming and outgoing multiplicities must agree: skipping a duplicate cannot
improve coverage. It retains terminal candidates as descriptors; increasing
terminal counts, reducing nominal four-bar overlap or removing an instrumental
bed does not automatically declare deterioration.

Risk review covers technical violations, vocal/held-note loss, missing foreground,
abrupt tonal removal and evidence coverage. Compare each occurrence as well as
totals when adjacencies and occurrence identities match. Inherited state changes
are reported explicitly. `review_required` requests inspection;
`incomplete_evidence` means unchanged missing evidence prevents a complete
comparison. `no_regression_detected` is never listening acceptance. The CLI's
`review_required` is true for either of the first two states.

Signal review also flags increased exposure at every existing rhythm, harmony,
voice, energy and foreground threshold, plus additional unknown exposure.
For unchanged adjacencies, compare each queue occurrence as well as the totals:
one pair's improvement cannot conceal another pair's added gap or conflict.
Longer legitimate overlap can increase these exposures too; the flag requires
inspection and does not declare that the transition sounds worse.

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

## Explicit cycles and duplicate playlist entries

A run may supply `evaluated_pairs`, `cycle_size` and `queue_entries`. The latter
is one `{entry_id, track_id}` per plan source plus the final successor. Entry IDs
identify playlist occurrences, so two entries of the same recording have distinct
IDs; the same occurrence repeats its ID on the next cycle. Cycle metadata is
validated against every plan and repeated entry sequence. `evaluated_pairs`
excludes any extra following context edges from counts while retaining their
position for solo-context accounting. Per-cycle reports label complete/partial
cycles and include the last-to-first handoff. Without this explicit metadata the
report says `sequence_only` and never guesses a cycle from unique track IDs.

Example run for two repeated occurrences of one recording, one full cycle and
one following context edge:

```json
{
  "name": "order",
  "plans": "plans.json",
  "evaluated_pairs": 2,
  "cycle_size": 2,
  "queue_entries": [
    {"entry_id": "playlist:0", "track_id": 7},
    {"entry_id": "playlist:1", "track_id": 7},
    {"entry_id": "playlist:0", "track_id": 7},
    {"entry_id": "playlist:1", "track_id": 7}
  ]
}
```
