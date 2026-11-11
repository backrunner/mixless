# Search coverage and upstream analysis audit

Date: 2026-11-11. Production order version remains **26** and analysis version
remains **26**. The experimental candidate expansion described below was
rejected and removed. Earlier exact-output performance changes are retained.

The subsequent version-27 fixes and final regression review are recorded in
[the repair follow-up](2026-11-11-planner-repair-and-review.md). The version and
results below describe this earlier audit checkpoint.

## Decision

There is a real search omission, and a higher planner score is insufficient to
approve its repair. Enumerating additional whole-bar lengths discovered a safe-
according-to-current-gates nine-bar DnB transition that the usual duration pool
missed. A general 4–16-bar expansion selected it, but cross-playlist observation
also exposed increased foreground gaps and vocal-risk flags. The experiment was
therefore reverted without weakening gates, changing score weights, or tuning
rules to individual tracks. No changed musical policy is enabled in the app.

The retained work is an offline coverage reference, an exact small-matrix
reference, source/reanalysis auditing, and independent PCM regressions. It does
not certify exhaustive musical search or accurate labels for arbitrary music.

## Search boundaries checked in current source

| Layer | Current boundary |
| --- | --- |
| Adjacency | All directed pairs for 3–40 tracks; 12 rough successors per track for 41–200; rough ordering above 200 |
| Order | Up to 16 starting tours and relocation of segments of 1–3 tracks; heuristic rather than exhaustive tour enumeration |
| Stateful repair | At most four chain simulations for small sets, including closing context |
| Anchors | Ranked ordinary anchors plus protected vocal, recovery and peak anchors; ordinary pool truncates to eight |
| Lengths | Selected conventional and evidence-derived whole-bar lengths, with additional sampling for dense pools; fixed PhraseBridge list |
| Arrangement | Bounded part/window alternatives and local fitting; not every possible automation envelope |
| Anticipation | Best plan per entrance, a shortlist of three plus native/held alternatives, one following track |

The new ignored `smooth::coverage::compare_search_coverage` test can remove the
ordinary anchor truncation, remove duration sampling, disable cross-window score
pruning, and forecast every retained per-entry best. `MIXLESS_COVERAGE_ALL_SPANS`
also enumerates every integer length accepted by the mode's span limits, through
128 bars. Optional previous-plan input reproduces carried entry/rate/pitch and
earliest departure. Four input tracks additionally compare two later departures
using actual carried state and the production planner.

This reference still preserves musical eligibility, manual cue authority,
pair-internal plain/ride selection, the arrangement fitter and one best per
entrance. It does not enumerate all fractional start times, every part routing,
all future alternatives, or every playlist permutation. The deeper rollout is
a sensitivity check under a discounted objective, not a global optimum proof.

## Measured search coverage

Fresh exports came from a consistent backup of the current desktop library.
Nine cases covered the four existing genre playlists, one carried state, no-
stems playback, and four two-departure rollouts. All completed. The reference
evaluated **73,711 bounded windows and 222,227 expanded windows**. These are
attempts, not distinct audible successes or listening judgments.

No score-pruning discrepancy was found on the evaluated bounded candidates.
The expanded reference found an improvement in the 106→107 DnB case: zero
feasible default-search windows became 17 feasible windows when integer spans were
included. The best immediate candidate used nine bars. Widening entrances and
subsequent-state evaluation also found a lower two-departure cost by about
0.3423. The 110→108 case remained infeasible in its evaluated expanded pool;
the other cases did not improve the measured objective.
Native/held alternatives already supplied a production plan for 106→107;
the empty default-search pool does not mean the whole production planner failed.

The exact Held–Karp example was checked against brute-force permutations for
2–7-node directed matrices. On the saved 11-track audit, initial and final
repair-round matrices had zero heuristic gap; round 1 had cost 7.742608 against
the exact 7.697974, a **0.58% gap**. Current playlist payloads were verified equal
to that saved audit's inputs before reusing it. A matrix optimum does not prove
an optimum of the carried-state chain or of audible quality.

## Rejected expansion and cross-playlist checks

The experiment added every integer BeatBlend/LoopRoll span from 4 through 16,
preserving the existing longer evidence-derived spans, safety gates, ranking
and lookahead. It temporarily used order version 27 in the experimental binary.
That version and the production span change were reverted.

Four full fixed cyclic queues produced **80 plans**, including closing and
revisited first departures. Twenty-five plans changed. Baseline exports were
verified equal to the fresh library inputs. Observer v8 evaluated both variants
against the same analyses, with 1/2/5% audibility and vocal-grouping sensitivity.
All four comparisons requested review. Selected diagnostics:

| Playlist | Plans | Vocal-risk pairs before → experiment | Foreground-gap seconds before → experiment |
| --- | ---: | ---: | ---: |
| Melodic (3) | 12 | 0 → 0 | 2.25 → 5.35 |
| Electronic (4) | 23 | 1 → 2 | 4.65 → 5.60 |
| MUG (5) | 30 | 2 → 2 | 4.85 → 3.70 |
| DnB (7) | 15 | 1 → 2 | 3.60 → 3.60 |

These are observer diagnostics, not human-annotated phrase damage. They are
enough to reject an assertion of unchanged or improved quality, and they show
why a local score improvement must not be accepted in isolation. No production
threshold was changed to make these reports pass. Fresh Best was not rerun for
the rejected policy; these results describe fixed cyclic queues only.

The original and experimental 106→107 plans were rendered with the actual
cached four-stem PCM, four seconds of pre-roll and eight seconds of post-roll.
Both completed at the expected carried pitch/rate. Independent WAV scans found
48 kHz stereo float output, no nonfinite or clipped samples, and peaks 0.5191
and 0.5483. Actual simultaneous deck selection was 9.152 s and 5.349 s,
respectively: the nominally longer plan did not provide more simultaneous
playback. No physical playback or listening acceptance was performed.

## Upstream evidence

- The current backup contains 103 analyses: 78 at version 26 and 25 at version
  23. All 76 entries across the four playlists use version 26.
- Full-file BLAKE3 verification passed for all 78 available current sources.
  The 25 old-version records have missing files and are outside these playlists.
  Database source/analysis hashes matched. Beat ordering and feature coordinate
  range checks found no violations. There were no manual cues in this snapshot.
- Four recordings, one per playlist, were decoded and reanalyzed with current
  code and their existing stems. BPM, key labels, beat positions and section
  boundaries were unchanged; some floating-point confidence/spectral features
  differed. This is reproducibility evidence, not independent musical truth.
- The same four decoded recordings were independently analyzed after a 0.5
  gain and approximately 317 ms leading shift, with the optional native voice
  classifier disabled in both control and transformed runs. Key labels were
  unchanged, maximum BPM difference was 0.0162, and beat P95 alignment error
  after removing the known shift was 0.82–14.02 ms. Existing stems were not
  reused for the shifted PCM.
- Known PCM fixtures establish their own pulse and accent timing. At 90,
  127.5 and 174 BPM, across 22.05/44.1/48 kHz, gain and offset variants, 4/4
  beat/downbeat P95 errors were below 16 ms. Separate synthesized triads at
  four transpositions recovered the expected three pitch classes. This checks
  chroma extraction, not the ambiguity of global musical key labels.
- **Meter remains a support gap.** Generated 3/4, 5/4 and 7/4 fixtures still
  receive meter 4/4. Their maximum bar confidence was at most 0.203 while pulse
  confidence remained 1.0. A regression now protects low downbeat confidence,
  but this does not amount to correct meter detection or establish every
  planner consumer handles that uncertainty correctly.

The four playlists have already been used for development and regression.
There is no newly annotated independent corpus in this audit for true phrase,
section, vocal, meter or local-key accuracy. Agreement with stored results and
stability under transforms cannot fill that gap.

## Validation and reproduction

- Workspace: **766 passed, 30 ignored** with
  `cargo test --locked --workspace --exclude mixless-desktop`.
- Exact matrix reference: one additional example test passed.
- The ignored PCM audit was explicitly run and passed after its final cleanup.
- `./dev.sh --build` passed and refreshed the dev app bundle without launching.
- Final production binary: **8 complete plans were byte-identical** to the
  pre-task binary across three genre excerpts and no-stems playback.
- Only audit/test accessors and unchanged bounded generator wrappers remain in
  production source. No audio, personal library, database or render is committed.

All local evidence is under `target/planner-coverage-20261111/`, including
`coverage-summary.json`, `exact-matrix-reference.json`, `payload-check.json`,
`source-check/`, `transform-check/`, `observer-comparisons.json`,
`render-validation.json`, `final-parity.json` and build/test logs. The rejected
binary and duration implementation are retained there for reproduction.

Run the references with absolute input/output paths because Cargo test runs in
the crate directory:

```sh
MIXLESS_COVERAGE_INPUT=/absolute/analyses.json \
MIXLESS_COVERAGE_OUTPUT=/absolute/coverage.json \
MIXLESS_COVERAGE_ALL_SPANS=1 \
cargo test --locked --release -p mixless-mixplan --features audit \
  smooth::coverage::compare_search_coverage -- --ignored --exact --nocapture

cargo run --locked --release -p mixless-mixplan --features audit \
  --example order-matrix-audit -- /absolute/order-audit-directory

cargo run --locked --release -p mixless-analyze --example analysis-evidence-audit \
  -- /absolute/library-copy.db /absolute/output-directory TRACK_ID

MIXLESS_ANALYSIS_VALIDATION_OUTPUT=/absolute/known-pcm.json \
cargo test --locked --release -p mixless-analyze \
  audit_known_pcm_meter_and_transformations -- --ignored --nocapture
```

Further policy changes need to resolve the disagreement between the candidate
score and foreground/vocal evidence before accepting a wider search. Meter
support and independently annotated musical events require separate validation.
The current findings do not justify narrower candidate pruning for speed.
