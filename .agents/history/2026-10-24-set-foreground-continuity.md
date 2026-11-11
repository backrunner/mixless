# Set foreground continuity audit — 2026-10-24

The reported Electronic set is a regression corpus, not an untouched evaluation set. No listening-quality acceptance is claimed.

**Correction from the later [full AutoMix audit](2026-10-24-automix-full-audit.md):** the continuous renderer used for this record did not restore a reused deck's channel fader. The old `validated-continuous.wav` is silent after the early handoffs (verified at 500, 1000 and 2000 seconds). Its duration, peak and completed source clocks do **not** establish continuous audible playback. Individual pair renders and planning measurements are separate evidence. The later audit repairs the renderer, adds per-handoff signal checks and replaces the continuous-render evidence.

## Evidence and reproduction

- Frozen pre-change working-tree source, executable and diff are in `target/set-regression-20261024/source-before`, `planner-before` and `before.patch`. Committed HEAD was also compiled separately against the same inputs; it already admits the three problematic adjacencies, so the entire regression cannot be attributed to one recent energy formula.
- The SQLite backup contains 22 current analyses at version 26 for the reported set. All 22 source file hashes match. Across the library, 53 available recordings match their stored metadata-independent audio identities; 25 original files are unavailable. The other 56 cached analyses are version 23 and their planning runs are legacy/missing-evidence regressions, not validation of current analysis quality.
- The reported order was replayed with carried entry, rate, pitch and earliest departure, including the closing transition and the following departure. Input arrays, raw plans, source hashes and numerical observers remain under the ignored audit directory.
- I’m With You → Fire Away used a compact DropCut at source 125.82 s into 72.06 s. The last outgoing two bars measured about -7.48 drive units versus -3.08 for the incoming bars; the outgoing natural continuation was about -5.67. Actual normalized PCM increased about 3.21 dB in the sampled before/after windows. These units are not interchangeable with perceptual energy.
- Fire Away → Ice Shards removed the outgoing melodic foreground after about 24.7 source seconds of established playback. The incoming melody rose in about 0.56 wall seconds.
- Ice Shards → Spiritbond faded the melodic stems in about 0.55 wall seconds. The outgoing removal was around 174 s, away from its observed release, while the incoming reveal was inside a build. Keeping total power did not preserve that phrase.

## Changes

- `energy::unearned_rise` compares a compact cut with both preceding local bars and the source’s natural continuation. A Drop label alone cannot exempt a surge. Explicit cues and terminal handoffs retain their authority; missing measurements do not assert a surge.
- `melody` fits sustained melodic exchanges to observed outgoing releases and incoming phrase entrances. Two-to-four-local-bar envelopes are a mixing policy, not a new phrase detector. A measured negligible instrument may move quickly. Final routed-envelope validation also covers legacy stem plans. Existing sung-rest and held-note checks remain active.
- Phrase fitting chooses the nearest feasible exchange after checking the incoming peak deadline, including a shorter fade when the longer alternative cannot arrive in time. Experiments that broadened the fit indiscriminately or charged the preceding ordering edge were not retained after whole-chain comparisons.
- `established_entry` carries the actual incoming envelope into desktop AUTO, preview sequencing, pitch-recovery lookahead and ordering. Muted playhead motion and a drum pickup cannot count as a fully heard track. Foreground departure, not a residual lane’s final timestamp, contributes to residence scoring.
- A short complete peak reduces but does not erase residence cost. The cost of continuing into another peak is discounted when the preceding foreground passage was short, removing a conflict that favored premature departures.
- BEST’s bounded chain audit compares actual costs with the pair forecasts, repairs rushed passages as well as emergency handoffs, and revisits the first departure after closing the loop. `ORDER_VERSION` is 7 so persisted version-6 results are not reused.
- The continuous render example reads the same metadata-independent stem identity as desktop, with a full-file legacy-cache fallback. Production callback/DSP code was not changed by this work.

## Verification and boundaries

- Planner: 186 tests pass, one explicitly ignored; counterexamples include real build resolution versus unearned rise, independent mastering gains and tempos, a silent voice over a sustained synth, absent instruments, gradual phrase releases and incompatible keys.
- Full workspace tests excluding desktop pass with one test thread. An earlier parallel run hit the existing native HID discovery timeout in two controller tests; no audio threshold was changed to hide that failure.
- Final desktop AUTO tests passed 45 tests with three explicit ignores; the development bundle builds successfully. The task did not launch, restart or install the app and did not commit or publish changes.
- The final selected loop has 22 unique track occurrences, five compact cuts, zero emergency handoffs, zero overlapping schedules and zero foreground passages below 30 source seconds across the 23 checked transitions (one circuit plus the first repeated departure). These counts establish planning properties, not listening quality.
- Spiritbond and the late-entered Aeipathy still have approximately 37 s and 32 s of foreground residence. They remain explicit listening-review points; completing a detected peak does not prove either duration feels right.
- Holding the old reported order fixed produces five emergency handoffs under the stricter checks. It should not be presented as a good set merely because every pair previously returned a plan. The revised BEST order changes the problematic adjacencies.
- Three reported pairs were rendered through actual stem PCM. The old continuous harness reported 23 handoffs over 2662.41 seconds with matching source clocks, but **that continuous-audio conclusion is withdrawn**: it left reused faders closed. Use the corrected renderer and final artifacts in the later full audit; neither run establishes physical-device listening acceptance.

## Artifacts

`target/set-regression-20261024/final-manifest.json` fingerprints the final source and frozen planner. `validated-loop-plans.json` and `validated-loop-metrics.json` contain the final chain; `validated-reported-*` retains the fixed-old-order comparison. `validated-continuous.wav` and its log are retained only as invalidated historical evidence, not audition/continuous-playback acceptance. Personal recordings, SQLite copies, stem caches and generated audio remain untracked under `target`.
