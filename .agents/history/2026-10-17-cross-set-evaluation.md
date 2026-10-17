# Cross-set AutoMix evaluation

## Result and scope

The changes have no recording, title, playlist or reported-timestamp exceptions
in production decisions. Improvements in vocal attenuation and several harmonic
and energy diagnostics extend beyond MUG. **The candidate is not uniformly better:**
audible layering becomes shorter in every inspected set, and some Best-order
harmonic/energy measurements regress. Do not describe this as complete musical
acceptance or proof against overfitting.

This change adds a reusable independent observer, matrix generator, documentation
and CI tests. It does not retune the production algorithm to make this corpus pass.
The frozen hypotheses and formulas are in [the observer specification](../../scripts/automix_eval/README.md).
The observer measures the combined source clocks, rate/pitch, stem gains, channel
levels, equal-power crossfader, EQ and logarithmic filters. Candidate `score` is
never treated as an independent quality metric.

## Evidence

- Baseline: `4b5671c`, before the reported-case optimization series.
- Candidate: `6c251d85b17c862d0b55083210beee31715793f0`.
- Additional control: baseline planner with the candidate's fully reanalyzed data.
- Fresh SQLite backup matches the earlier snapshot's tracks, analysis payloads,
  playlist membership/order and cues. All **77** current source BLAKE3 hashes
  match. No manual cues are present. The live library was not written.
- Old analyses: 75 at version 20, two at version 15. New analyses: all 77 at
  version 21, with source-aligned stem evidence attached once.
- DnB: 14 tracks / 13 transitions; Melodic Dubstep: 11 / 10;
  Electronic: 21 / 20; MUG: 29 / 28. Downloads: two tracks / one transition,
  reported separately from the four-set macro averages.
- Each variant uses both the saved fixed order and a freshly generated Best
  order: **30 runs, 432 transitions**. Best comparisons are policy comparisons;
  changing the pairs prevents a pairwise causal interpretation.
- All recordings have been inspected during development. Alternate encodings
  remain in the same regression corpus. There is no untouched holdout and no
  inference about untested pop, acoustic or other arrangements.
- Both binaries were freshly built from their own source revisions. Separate
  Cargo target directories avoid a stale shared example output path.

Binary SHA-256:

| Variant | SHA-256 |
| --- | --- |
| Baseline | `96493babf7617795390f0827807fffe34acbc6872f34a97ebd133807d7668fc9` |
| Candidate | `aa13f9949fdf06b0c58788c3a2e75b6ac16db650d0567d786173ffc254bb2f73` |

Private evidence is retained under ignored `target/generalization-review-20261017/`:
`matrix.json`, `evaluation.json`, `source-check.json`, observer/binary hashes,
individual plans/orders and build/test logs. Only `evaluation.json` is the final
observer result; provisional outputs preceded stem-specific chroma, segment-indexed
grid confidence and separate drum diagnostics. No audio, personal library or
generated analysis is committed.

## Per-set results

Values are baseline → candidate. Audible overlap requires both decks to exceed
2% of their own active reference power. It excludes silent preparation and solo
pitch recovery. Seconds below are totals over observed transitions, not nominal
scheduled bars. Risk percentages use their respective observed eligible exposure,
not total track duration. Roughness is a note-interval proxy, not a probability
of perceptual dissonance. Voice attenuation is not the same as vocal-boundary
truncation. Energy estimates include dry signal only.

### Fixed order

| Set | Audible overlap (s) | Zero-bar cuts | Played out | Roughness > 0.4 | Voice attenuation > 6 dB | Ongoing-peak deficit > 6 dB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| DnB | 42.0 → 17.6 | 0 → 0 | 2 → 6 | 46.5% → 37.4% | 19.3% → 0.0% | 4.1% → 0.1% |
| Melodic Dubstep | 44.2 → 20.8 | 0 → 0 | 3 → 2 | 44.3% → 43.9% | 10.6% → 0.0% | 0.0% → 0.8% |
| Electronic | 48.9 → 44.3 | 1 → 6 | 3 → 5 | 42.1% → 29.5% | 18.9% → 0.0% | 5.3% → 0.6% |
| MUG | 64.6 → 27.8 | 4 → 9 | 9 → 7 | 47.5% → 40.5% | 7.3% → 0.0% | 10.6% → 0.4% |

Four-set macro averages: roughness exposure **45.10% → 37.82%**, voice
attenuation **13.99% → 0%**, peak deficit **5.01% → 0.47%**. Total observed
overlap falls **199.6 → 110.6 s**, cuts increase **5 → 15**, and played-out
transitions increase **17 → 20**. These losses cannot be hidden by larger
scheduled plan lengths or lower conflict totals.

### Fresh Best order

| Set | Audible overlap (s) | Zero-bar cuts | Played out | Roughness > 0.4 | Voice attenuation > 6 dB | Ongoing-peak deficit > 6 dB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| DnB | 104.6 → 5.9 | 0 → 1 | 1 → 2 | 47.6% → 8.3%* | 7.6% → 0.0% | 11.5% → 13.0% |
| Melodic Dubstep | 76.9 → 32.4 | 0 → 0 | 1 → 1 | 30.7% → 39.8% | 56.0% → 0.0% | 14.9% → 0.0% |
| Electronic | 125.8 → 86.8 | 0 → 4 | 4 → 4 | 44.6% → 46.0% | 5.1% → 0.0% | 13.2% → 1.4% |
| MUG | 73.2 → 53.5 | 5 → 6 | 8 → 4 | 38.1% → 35.2% | 27.9% → 0.0% | 12.3% → 5.3% |

*Candidate DnB has only **0.60 s** of observed harmonic overlap. This number
cannot support a broad harmonic-quality claim. Four loops per variant are
excluded from dry observation in Best orders (distributed differently by set).
Observed overlap is therefore a subtotal, with unknown loop exposure kept separate.
The large DnB reduction also appears in its fully observed fixed order.

Best macro averages: roughness **40.24% → 32.33%**, voice attenuation
**24.15% → 0%**, peak deficit **12.95% → 4.93%**. Overlap subtotal falls
**380.4 → 178.6 s**, cuts increase **5 → 11**, played-out transitions fall
**14 → 11**. Aggregate improvement does not erase the DnB energy regression or
the Melodic/Electronic harmonic regressions.

Downloads has the same order in both modes: overlap **3.7 → 1.6 s**, zero cuts
in either version, played-out **0 → 1**, zero pitch offset. It is not counted as
a fifth genre-sized set in macro averages.

## Attribution, timing, pitch and progression

With fixed order, replacing only the analysis changes overlap **199.6 → 209.3 s**,
roughness macro **45.10% → 46.03%**, voice attenuation **13.99% → 13.02%** and
peak deficit **5.01% → 3.87%**. The large vocal-protection and shorter-layering
effects therefore come mainly from planning, not merely corrected tempo analysis.

Broad rhythmic-foreground exposure with reference-grid phase error above 30 ms
falls from **17.62% → 6.56%** in fixed order and **7.83% → 2.83%** in Best.
Those diagnostics include onset-bearing instruments and can flag weak pad
attacks. The remaining flagged native bridges have no simultaneous audible
drum groups at the default threshold. Separate drum-only exposure is small:
fixed order has no >30 ms error in either version; Best changes from **0.25 s**
to **0 s**. This is conditional grid consistency, not an independent guarantee
that every kick is aligned. The prior PCM pulse audit remains separate evidence.

The separate drum-activity-change proxy also finds a small regression: exposure
above 0.25 changes **0.15 → 0.60 s** fixed and **0 → 0.15 s** Best. Candidate
fixed-order flags occur in Electronic; Best flags occur in MUG. This diagnostic
includes source rhythm changes as well as control masking, so it is an inspection
trigger, not proof that the change is an inappropriate drum entrance.

Fixed-order harmonic evidence covers **141.7/170.4 → 73.5/96.2 s**;
Best covers **266.5/334.5 → 102.6/138.0 s**. Missing note evidence remains unknown.
All 432 plans pass source bounds, carried offset and adjacent-transition overlap
checks. Twelve loop plans across the three Best variants are explicitly excluded
from signal metrics. Send FX retain dry-only diagnostics; baseline uses these
more often, so dry deficit is not total audible deficit including echo tails.

The final audible outgoing voice edge still intersects a detected sustained
note in **3/45** observed fixed-order candidate pairs and **3/46** Best pairs,
down from **8/41** and **17/45**. Incoming note-crossing flags also remain.
Zero measured attenuation does not certify every lyric ending or entrance.

Mean absolute pitch exposure, in semitones, changes in fixed order:
DnB **0.14 → 0.48**, Melodic **0.09 → 0.85**, Electronic **0.04 → 0.49**,
MUG **0.00 → 0.37**. Corrective transposition increases pitch burden compared
with the baseline that often did not match keys. **55/71** fixed handoffs and
**51/71** Best handoffs finish at native pitch. There are two committed solo
restorations in fixed order and none in these Best orders. Maximum completed
offset is 3 semitones fixed and 6 Best. These results do not show a set locked
to its first track, but do not prove the recovery policy is optimal.

Using section labels only as a progression proxy, macro mean delay after the
first complete observed Drop changes **62.6 → 54.9 s** fixed and **50.8 → 47.6 s**
Best. Pairs with multiple complete Drops change **24 → 24** and **20 → 16**.
These labels do not independently establish a safe earlier exit. Cue event
recall remains unavailable without independent annotations; the previously
inspected Reborn entrance is a regression observation, not cue-accuracy truth
for all recordings.

## Sensitivity and validation

Audibility thresholds 1%, 2%, 5% preserve the overlap reduction:

| Order | Baseline overlap (s) | Candidate overlap (s) |
| --- | --- | --- |
| Fixed | 238.8 / 199.6 / 149.6 | 131.3 / 110.6 / 74.1 |
| Best | 468.1 / 380.4 / 248.6 | 213.0 / 178.6 / 124.4 |

At 3/6/9 dB deficit thresholds, fixed macro peak-risk exposure changes
**17.26/5.01/1.36% → 4.47/0.47/0.08%**; Best changes
**28.89/12.95/4.71% → 8.71/4.93/3.16%**. Voice attenuation remains zero in the
candidate at all three thresholds, with boundary-note flags reported separately.
At 20/30/50 ms, broad grid-risk macro changes **19.81/17.62/14.25% →
6.56/6.56/4.77%** fixed and **8.41/7.83/6.14% → 2.83/2.83/2.36%** Best.
No threshold was selected to hide a regression.

- Observer and matrix tests: **23 passed**. Coverage includes quarter-beat errors,
  half/double equivalence, gain/time/tempo transformations, per-segment confidence,
  quiet held voice, missing stems/notes, muted-part harmony, aligned-grid drum
  activity changes, natural attacks and
  releases, cuts, solo restoration, loop exclusions and snapshot provenance guards.
- Workspace: **567 passed**, 27 ignored. Desktop: **185 passed**, 7 ignored.
- Development bundle build passed, without launching or installing.
  Version `0.1.0-beta.4`, dev, revision `v0.1.0-beta.4-14-g6c251d8-dirty`,
  built `2026-10-17T15:00:39Z`, macOS 27.0.1 (26A434).
  Executable SHA-256: `11ab3cf7f40ed65eab1681d34cec21826dccce80848adda88d6c99f26953e308`.
- Previous production revision's two remote Rust checks passed. This evaluation
  change adds the Python observer tests to that workflow.

There was no new human listening, hardware playback or PCM rendering in this
evaluation-only change. Production DSP/callback code is unchanged. The evidence
supports cross-set improvements in specific diagnostics and identifies material
regressions in live layering; it does not support declaring all requested
listening outcomes solved.
