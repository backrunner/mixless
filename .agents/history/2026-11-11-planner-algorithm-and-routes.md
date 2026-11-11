# Exact power reuse and routing coverage

Date: 2026-11-11. Baseline: `2015c7ca7077eb948cf776f0d2314f7f8c15aaf2`.
The user authorized further algorithm optimization while preserving mixing
quality and improving the choice of suitable routes. This follows the
[larger-playlist measurements](2026-11-11-planner-scale-search.md).

## Exact-result power reuse

The foreground boundary check and five-sample bridge measurement now reuse the
existing immutable stem-power evidence. Both previously repeated dB-to-linear
conversions inside candidate evaluations. The same frames, sample order,
missing-evidence behavior and floating-point addition order remain intact.
Tests compare both paths bit for bit against their original formulas, including
frame gaps, track ends, missing stems, invalid query times, nested evidence
scopes and foreign tracks. Analysis extraction and persisted evidence do not
change.

## Rejected scoring experiment

An experimental width fitter carried the incumbent cost into scoring, retained
threshold-specific lower bounds separately from complete results, and reused
routed powers for unchanged gains. Its boundary and reference-formula tests
passed, but the matched 40-track full Best-order run did not establish a speed
benefit: 1004.264 seconds before versus 1013.775 seconds after (0.95% slower in
this single sample). Pair time was 537.971 versus 549.650 seconds; carried-state
chain repair was 466.271 versus 464.103 seconds. Total user CPU time was nearly
unchanged (11881.61 versus 11857.91 seconds). These results do not justify the
added scoring machinery, so that experiment was removed from the delivered
code. Its frozen binary, patch, source and measurements remain in ignored
artifacts. The experimental run included the candidate coverage repair below.

The experiment preserved the final 40-track order and all 1560 cold pair
results. Of 1725 audit files, four changed: two carried-state plans changed only
their summary score and sub-microsecond clock duration, with corresponding
round totals in two files. Every routing/control lane and source endpoint
remained unchanged. Full four-round repair still ran.

## Candidate coverage repair

Lookahead previously discarded a native/held alternative when its four source
endpoints and final performance offset matched a previous candidate. Those
values do not establish equivalent stem routing, EQ, control timing or score.
The alternatives have already been generated and passed their own safety gates;
they now reach the existing musical-cost comparison. There are still at most
the original entrance shortlist plus the two native/held alternatives.

Only the following departure forecast is shared, using the exact established
entry, end time, rate and pitch after tempo preparation. If every alternative
has that same carried state and already costs at least as much as the first
route, the first route wins without an unnecessary departure forecast. Equal
costs preserve traversal order, and failed plans cannot displace valid plans.
A counterexample test retains a better routing with identical endpoints while
computing its departure only once; it also checks ties, failures and the
dominated-state shortcut.

This correction can change the selected plan, so **ORDER_VERSION is 28**.
**ANALYSIS_VERSION remains 27**: analysis extraction, stored musical evidence,
manual cue authority, playback DSP and musical safety gates are unchanged.
The earlier rejected broad duration expansion remains disabled; a higher
planner score alone is not accepted as proof of better listening quality.

## Real-input comparisons

The frozen routing-experiment binary produces 84 plans across four existing
regression queues. Of these, 83 are unchanged.
One 102-to-101 cut retains every source endpoint, stem/EQ/gain/rate/pitch lane
and strategy. Its score changes from 0.47803786 to 0.47861263 and its clock
duration changes by approximately -0.23 microseconds. The independent observer
finds no new alerts or increased pair/signal exposure under 1/2/5% audibility
and 1/2/3-pulse vocal-grouping checks. It still reports `incomplete_evidence`
because some signal metrics have pre-existing observation gaps. That status
has not been suppressed or converted into full quality acceptance.

That experiment also runs the selected 64-track cycle and produces 66
carried-state plans. Of these, 65 are
unchanged. The reverse 101-to-102 cut retains the same controls and strategy;
its score changes from 0.4818887 to 0.4823202 and clock duration by approximately
+0.36 microseconds. Neither set adds a failed plan. These are existing
regression recordings, not newly annotated or held-out audio.

Both changed cuts were rendered from their real source recordings with four
seconds of pre-roll and eight seconds after the handoff. Each before/after pair
produces **byte-identical WAV files**. These plans use the full mix and require
no stems. Independent scans find 48 kHz stereo float output, zero nonfinite or
clipped samples, and peaks of 0.490899 and 0.505879. The engine runs offline;
no audio device or running app is involved. This establishes unchanged rendered
audio for these cases, not a subjective listening improvement. The demonstrated
quality benefit is repairing an incorrect candidate exclusion; the real corpus
does not establish an audible quality gain.

The final 40-track run changes two more carried-state plans versus the original
baseline, again only in summary score and sub-microsecond clock duration. Both
real-audio before/after renders are byte-identical too. All four additional
WAV scans report zero nonfinite or clipped samples. Thus all four observed
changed transition contexts have exact rendered-audio parity.

## Search coverage and evidence limits

The 40-track input uses 40 distinct real analyses and exercises all 1560
ordered cold pairs plus four carried-state repair rounds. The 64-track input
interleaves the four existing regression playlists, including the DnB set,
and exercises the existing 12-successor policy (768 planned pairs). It does
not run carried-state order repair, as prescribed by the unchanged >40-track
policy. These two sizes therefore have different computational workloads;
their elapsed times are not a scaling curve under a single search policy.
Beyond 200 tracks the existing ordering path still uses rough costs only.
No claim of exhaustive global optimality follows from these tests.

This change neither reduces nor broadens those limits. It corrects a route
exclusion inside the existing lookahead and reduces repeated work on the same
measured evidence. It does not prove that every tempo, phrase or instrument
analysis is correct: these inputs are frozen prior regression analyses, not a
fresh independently annotated music corpus. The existing analysis/version and
fallback/safety rules remain unchanged.

## Validation

- Final workspace tests: 781 passed, 30 ignored. Trace mode: 280 passed,
  8 ignored. Release planner tests: 285 passed, 2 ignored. Planner Clippy
  completes with existing style warnings. Formatting and diff checks pass.
- `./dev.sh --build` refreshes the development bundle without launching it.
- The final rebuilt binary reproduces all 13 plans from the routing experiment's
  eleven-track cycle and all four original no-stems plans byte for byte.
- Each timed process runs without concurrent builds, audits or renders from
  this task. The unrelated user workload is left untouched.

The final 64-track run takes **415.292 seconds**, compared with the previous
same-input `2015c7c` run at **433.936 seconds** (4.30% less elapsed time).
All 769 audit files and the final order are identical. User CPU time changes
from 6373.18 to 6327.83 seconds (0.71% less), and maximum RSS from 718307328 to
664190976 bytes. This is a single-run observation against the prior same-day
baseline, not a fresh adjacent before/after repetition or a stable 4.3% promise.
The smaller CPU reduction means scheduling variance may explain part of the
elapsed-time difference. Both runs use 64 distinct tracks and the same search
budget; no persistent pair cache is supplied.

The fresh 40-track baseline takes **1004.264 seconds** and the final binary
**999.271 seconds** (0.50% less elapsed time, effectively flat in one sample).
Pair time is 537.971 versus 536.939 seconds; chain repair is 466.271 versus
462.309 seconds. User CPU time is 11881.61 versus 11828.32 seconds (0.45% less).
Maximum RSS is 1072644096 versus 1062453248 bytes. The final order is unchanged;
all 1725 audit files match the frozen routing/scoring experiment, confirming
that removing the experimental scorer and using shared powers preserves its
results. Compared directly with the original baseline, the only four changed
audit files are the two described plan summaries/clocks and two round totals.
All 1560 cold-pair files are byte-identical, and all four repair rounds run.

These measurements support retaining the small exact-power reuse and the route
coverage fix. They do not establish a substantial algorithmic speedup. No
search budget was reduced to improve these numbers.

## Evidence

Ignored artifacts are in `target/planner-algorithm-20261111/`: frozen baseline,
performance-only, routing-experiment and final binaries; source/input hashes;
unit and workspace logs; complete order audits; serialized queue plans;
observer comparisons and real-audio renders. Personal analyses, audio, database
snapshots and generated outputs are not committed. The live library and running
application are not modified.
