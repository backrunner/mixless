# Planner repair and submission review

Date: 2026-11-11. This follows the [coverage and analysis audit](2026-11-11-search-coverage-and-analysis.md).
The baseline is the version-26 working tree at task entry, including the earlier
exact-result optimizations, not Git HEAD. Analysis and ordering versions are now
**27**. The user authorized fixing the findings, reviewing the accumulated work,
and submitting focused commits with signoff.

## Retained fixes

- Plain and ride variants now compare the same final audible merit used by the
  outer search. A raw summary score could previously prune a better nonterminal
  arrangement before terminal/short-cut deductions were applied. The ride path
  also passes the same genuine-overlap check. Regression tests cover a higher
  raw score losing to a better audible arrangement and external score bounds.
- Whole-bar supplementation is a recovery path. Automatic lookahead exhausts
  the existing ordinary, native-key, held-tempo and prepared-handoff choices
  before trying missing BeatBlend/LoopRoll spans from 4 through 16 bars. Existing
  viable plans retain precedence, explicit strategies are unchanged, and all
  fitting and musical gates remain in force. Expanded results do not enter the
  bounded candidate memo or the cold-pair score cache.
- Meter detection compares phase means, repeated accents and separation among
  3/4/5/7-pulse hypotheses. An incomplete final bar cannot win by observation
  count alone. Odd meters also need agreement between both halves. Weak or
  contradictory evidence retains a four-pulse hypothesis with low confidence;
  it does not claim a reliable downbeat. Synthetic decoded-PCM regressions cover
  3/5/7 at 90, 127.5 and 174 BPM, plus four-pulse gain, shift and sample-rate
  variation. Accent counterexamples cover silence, alternating/uniform pulses,
  isolated impacts and changing meter.

Earlier exact frame lookup, scoped evidence reuse, request memoization and
directional persistent pair caches remain intact. This repair does not reduce
candidate budgets or chain-repair rounds to obtain a shorter runtime.

## Rejected alternatives

Expanding whenever only the default pool was empty was too early: native/held
alternatives could already supply a viable plan. The experiment replaced an
existing MUG handoff and increased its foreground gap by 0.4 seconds. The DnB
nine-bar alternative also increased roughness and foreground competition in
the observer. A proposed comparative proxy-quality guard was removed as well.
The final policy supplements only after the existing planning paths fail.
No quality threshold was relaxed to make these comparisons pass.

The DnB coverage reference still demonstrates the omission: 4,705 bounded
attempts have no feasible default window, while 37,400 expanded attempts find
17 feasible windows across nine entrances. The supplemental helper finds a
nine-bar arrangement. The full final planner keeps its already viable original
alternative. These are successes under current gates, not listening judgments.

## Current source and analysis evidence

All library work used a consistent SQLite backup. The running application,
live library, manual cues and audio files were not modified or restarted.
All 78 current analyses have valid source hashes and were recomputed in the
copy with version 27. All retained their four-pulse meter interpretation; seven
had small floating-point BPM/grid differences. The 25 older missing-source
records remain unavailable and are not counted as successful checks.

Four recordings, one from each regression playlist, were independently decoded
and analyzed again: serialized analysis matched the fresh version-27 copy.
On real decoded PCM with gain 0.5 and a 0.317-second prefix, DSP-only beat P95
errors after removing that shift were 3.79, 4.42, 6.28 and 4.48 ms. Key labels
were unchanged. Stems were not reused or shifted for this transformation test.
This checks identity, repeatability and selected invariances, not annotated
musical ground truth. The recordings are regression material, not a holdout.

## Full fixed-queue comparison

Four playlists contain 76 entries. The runs include a closing transition and a
revisited first departure; one further plan supplies equal following context.
There are 84 generated plans and **80 evaluated transitions**. Baseline tail
plans were regenerated with the version-26 executable to give both sides the
same context. In particular, the DnB baseline has two vocal-risk pairs under
this context; the earlier audit's shorter tail had one.

All 84 retain the same strategies and nominal exit times. Full plan comparison
finds no nonnumeric or array-length changes. The largest nominal source endpoint
difference is 0.118 ms; a wall-clock curve vertex differs by at most 0.130 ms.
Observer v8 uses the
same version-26 reference analyses for both sides, with 1/2/5% audibility and
one/two/three-pulse vocal grouping sensitivity.

| Playlist | Evaluated | Vocal-risk pairs before/after | Foreground-gap seconds before/after |
| --- | ---: | ---: | ---: |
| Melodic | 12 | 0 / 0 | 2.25 / 2.25 |
| Electronic | 23 | 1 / 1 | 4.65 / 4.65 |
| MUG | 30 | 2 / 2 | 4.85 / 4.85 |
| DnB | 15 | 2 / 2 | 3.60 / 3.60 |

Abrupt tonal-loss counts, sampled overlap and the observer's classification
counts also remain unchanged. No aggregate signal-regression alert was raised.
The comparison tool correctly retains incomplete-evidence/review statuses:
tonal observations have gaps, and exact pair comparison flags two tiny changes.
Manual inspection found a 0.00001575-second increase in one grouped vocal loss
observation, and a 0.00001548 increase in one instrument note's maximum
power-gain change per pulse. Both retain the same source event, risk category
and timing. Reports and thresholds were not edited to suppress these alerts.
Existing vocal-risk and terminal-handoff findings remain open for listening.

## Final Best-order timing

The fresh 11-track version-27 export completed Best order in **338.792 seconds**:
51.831 seconds for all 110 directed pairs, under a millisecond for tour search,
and 286.959 seconds for carried-state repair. Its order is
`[6,7,0,10,3,2,8,1,4,5,9]`, matching the previous version-26 audit order.
No own builds, tests or other planning runs overlapped this measurement;
unrelated user applications remained active. There is no matched version-27
uncached baseline here, so this is a current latency measurement, not a new
speedup percentage. First-time chain repair remains the main bottleneck.

That chosen order was also replayed with both immutable executables and their
respective analyses: 13 plans per side, 12 evaluated departures with equal
following context. Strategies and nominal exits match; incoming endpoints
differ by at most 0.061 ms. Vocal-risk pairs remain 2, foreground-gap exposure
remains 0.50 seconds and abrupt tonal-loss pairs remain 2. There is no aggregate
signal alert. The strict event comparison flags one unchanged vocal episode
whose loss fraction differs by 0.000000041; lost voiced time decreases by
0.00000239 seconds. This numeric review does not resolve the two pre-existing
vocal-risk findings or constitute listening acceptance.

## Review and integration

Review covered exact cache keys and lifetimes, deterministic worker reduction,
carried-state repair, cue authority, version invalidation, planner eligibility,
audio automation/manual takeover, queue occurrence identity, stale publication,
disconnected workers, library downgrade/backup, resource admission, HTTP/API
migrations and controller availability. A Clippy error in a local HTTP test
was fixed by checking the request read count. Stem progress text was also
updated to follow the repository's inline-separator rule.

- Required workspace tests: **770 passed, 30 ignored**.
- Desktop tests: **239 passed, 7 ignored**.
- Observer unit tests: **83 passed**.
- Controller C SDK check: **8 frames and 8 assertions passed**.
- Site content check: **24 pages, 136 search records, no errors/warnings**;
  Svelte checks also passed with no errors/warnings.
- `cargo clippy --locked --workspace --all-targets` passed, with existing style
  warnings; this is not a warning-free `-D warnings` result.
- `./dev.sh --build` passed. It builds the development bundle without launching
  or installing it. Existing UI screenshots were inspected as historical
  evidence, not represented as fresh interaction with this build.
- Changed Rust files pass rustfmt with their package editions; whitespace and
  submission-inventory checks exclude audio, libraries, secrets and audit data.

Release callback budgets passed all five tests on macOS 27.2 / Apple Silicon.
The dual-stem, pitch, FX, recording and cue workload measured 0.958 ms p99
against a 2.667 ms block deadline. No audio device or listening session was
used. Our planner/build/test workloads were serialized around this run, but
unrelated user applications and a training process remained active; this is
not a fully isolated machine benchmark or a hardware-playback acceptance.

Artifacts are under ignored `target/planner-repair-review-20261111/`: consistent
library copies, source and PCM checks, immutable audit executables, plans,
observer comparisons, rejected experiments and validation logs. Source audio,
databases and these artifacts are excluded from commits.

## Remaining boundaries

The planner still uses bounded anchors, arrangement fitting, shortlists and
four repair rounds. Large playlists still use sparse/rough pair evaluation.
The prior small-matrix Held–Karp and deeper-rollout audits remain useful
references, but neither certifies globally optimal carried-state planning.
Current upstream checks also do not independently annotate every beat,
downbeat, key, section or lyric. No new listening, native interaction or
physical-device acceptance is claimed by this submission.
