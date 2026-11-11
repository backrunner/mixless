# DnB case-by-case transition remediation — 2026-10-25

This continues the [replacement-audio audit](2026-10-25-dnb-audio-followup.md). The user asked whether the remaining cases can be handled individually. The accepted change improves candidate selection and BEST adjacency; it does **not** establish that all three original pairs now sound good in fixed ORDER.

## Mechanisms and accepted changes

- The fitted part solver previously retained only the nearest outgoing and incoming melodic phrase window. It now considers up to four of each, deduplicates identical shapes, renders their controls, and checks final foreground preservation before selecting the fit. This exposes feasible alternatives without weakening voice, melody, peak or harmonic gates.
- Candidate ranking previously omitted the outgoing continuation lost through the final stem, EQ, filter, gain and crossfader combination. `energy/arrangement.rs` now compares the actual routed result with that continuation, normalized to each recording's own active power. Complete local bars suppress single-kick noise; a long power window was rejected because occasional build impacts concealed the quiet bars between them.
- The soft loss budget varies continuously from 3 dB for a weak replacement toward the existing 6 dB part-exchange budget when the incoming material is full. The average does not include an untouched outgoing pre-roll. The next incoming bar is also compared against the outgoing natural next bar. Natural breakdowns remain eligible, absent evidence does not invent a loss, loops remain outside this new estimate, and manual OUT retains authority.
- The rendered loss participates in choosing a fitted arrangement and is deducted from its score only once. New functions separate deck powers for normalization; playback gain is unchanged. `ORDER_VERSION` is 9 so persisted BEST orders are invalidated. Analysis version remains 26; no key-lock DSP or callback code changed.
- Added regression coverage checks independent mastering gain and tempo changes, an ongoing verse versus a weak entry, a natural quiet release, missing evidence, routed EQ loss and sparse build impacts. Existing filtered-rhythm and incompatible-key drum-layer tests remain intact.

This is a general rule change. There are no song IDs, titles, source-second exceptions or per-playlist thresholds in production code. The optional ignored `smooth::audit_tests::enumerate_feasible_arrangements` test provides repeatable private-corpus enumeration without score pruning.

## The three cases

### Lost & Found → Flicker

The original 32-bar window runs A 165.572–209.707 s against B 2.794–46.930 s. The outgoing foreground leaves near A 187.6 s while Flicker is only starting its build around 24.86 s. The outgoing passage remains energetic even though its latter part is labelled Verse, so a peak-only guard misses the prolonged weaker replacement.

The unpruned search under the carried BEST context finds only one feasible arrangement with the existing voice, tonal and phrase evidence. Keeping that fixed adjacency still exposes approximately 20.85 s above the observer's 3 dB deficit screen, versus 20.8 s before; **this original pair is not repaired by the accepted change**. Adding more incoming section starts was tested, but did not provide a better legal pair and was not retained.

Recomputed BEST instead uses **Lost & Found → Feel This Good**. It introduces B at 156.100 s and finishes A at 233.154 s, with B's later complete drop ahead. The observer's >3 dB exposure falls to **8.2 s**, but there is still **2.35 s above 6 dB**, **0.7 s above 9 dB**, and a **10.75 dB maximum**. This is a shorter sustained loss with a deeper transient than the old pair, not a completed musical-quality fix. Review `lost-found-new-neighbor.wav` before accepting this edge by ear. Increasing the worst-bar penalty was rejected because it displaced a valid filtered-rhythm blend with a cut; its failing counterexample is retained in the private logs.

### Underwater → Orbit

The original four-bar blend runs A 137.962–143.478 s into Orbit's 0.073–5.589 s opening. That incoming opening stays quiet after its initial attack. The independent observer reports a maximum 11.81 dB deficit, and the original PCM confirms the quiet destination.

When the old adjacency is forced, the accepted planner chooses a terminal cut at approximately 212.439 s instead. Its deficit exposure is no longer measurable under the observer's active-outgoing screen, but waiting until the source end is **not** treated as a satisfactory musical repair or counted as a successful disappearance of the problem.

BEST instead chooses **Underwater → Flicker**, using Flicker's 35.896 s build phrase and a four-bar blend. This edge has **0 s above 3 dB** and a **1.38 dB maximum** over the same 0.55 s active-outgoing exposure screen, versus 11.81 dB before. There is no schedule or carried-state violation. This short eligible interval is not a full-transition listening verdict. Review `underwater-new-neighbor.wav`; phase agreement and energy estimates do not certify the musical phrasing by ear.

### Orbit → Feel This Good

The original pair has no feasible normal candidate under the audited carried context and falls back to a terminal one-bar handoff at A 235.910 s, after Orbit's own fade. Of the other 13 DnB successors tested in that context, Heaven supplies a normal stem arrangement. The bounded phrase-fit alternatives make that route available without removing vocal or tonal checks.

BEST now uses **Orbit → Heaven**, a **16-bar stem transition** over A **209.706–231.773 s**, entering Heaven at **23.487 s** and completing at **45.554 s**. This avoids the terminal fade; its measured >3 dB exposure is **0.4 s**, with none above 6 dB. The entire loop has zero emergency handoffs. Review `orbit-heaven.wav`.

This edge also has the new set's highest *instantaneous* note-roughness proxy, approximately 1.0, although its mean is 0.307. That proxy is sparse and does not establish a sustained harmonic clash or a clean listening result. The algorithm retains measured harmonic constraints and a maximum absolute carried shift of two semitones in this DnB run; key lock by itself is not proof of harmonic compatibility.

## Complete DnB comparison

All runs carry established entry, earliest staging time, rate and pitch across 15 handoffs, including the cycle closure and repeated first departure.

| Run | Zero-bar cuts | Emergency handoffs | >3 dB deficit | >6 dB deficit | >9 dB deficit | Max deficit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Before BEST | 3 | 1 | 23.30 s | 1.80 s | 0.40 s | 11.81 dB |
| Accepted planner, old BEST adjacency | 4 | 1 | 22.80 s | 1.25 s | 0 s | 7.65 dB |
| Accepted planner, recomputed BEST | 1 | 0 | 10.55 s | 2.35 s | 0.70 s | 10.75 dB |
| Before and accepted current ORDER | 5 | 2 | 0.15 s | 0 s | 0 s | 3.79 dB |

There are no defined source/schedule/carried-state violations or foreground residences below the 30-source-second screen. Minimum residence is 59.22 s in the BEST comparisons and 44.31 s in ORDER. One loop path per run is excluded from unsupported signal diagnostics; unavailable observations are not perfect scores.

No active-voice attenuation above 3 dB is measured on supported paths. Recomputed BEST has three outgoing note-boundary crossings versus two before: Punching Holes → Take Control, Feel This Good → We Are The Energy, and Flicker → Loving, Loathing. These remain review signals, not proven syllable cuts. Mean absolute pitch burden rises from 0.203 to 0.436 semitones; maximum remains two. Tonal-proxy exposure above 0.55 rises from 3.85 to 4.4 s. Thus the change improves emergency handling and sustained energy loss, but does not improve every proxy or establish overall listening acceptance.

## Real engine rendering

The accepted planner's current ORDER, old BEST adjacency and recomputed BEST were rendered through the existing corrected `render-set` engine harness, using the 14 actual M4A sources and isolated four-stem caches. All 45 handoffs complete with audible input, open incoming controls, verified source/rate/pitch carry, finite PCM and no clipping.

| Run | Rendered duration | Peak magnitude | Near-zero runs of at least 100 ms |
| --- | ---: | ---: | --- |
| Recomputed BEST | 2015.20 s | 0.62058 | Final source tail, 2009.86 s onward |
| Old BEST adjacency | 2270.34 s | 0.69669 | Opening 0–0.22 s and final tail from 2270.22 s |
| Current ORDER | 2525.97 s | 0.55207 | Final tail from 2524.00 s |

The scan uses 20 ms peak bins below 1e-4. It found no interior run of five bins. Approximately 113.5 minutes of accepted-candidate audio were rendered. No physical output device, live desktop gesture or by-ear listening session was used; no live playlist, cues, database or running app was changed.

## Cross-playlist regression and checks

The other four available lists were recomputed in current ORDER, old BEST adjacency and newly computed BEST, with actual carried state. This adds 204 new transitions; the observer compares 136 frozen before transitions as well. Combined with DnB there are 249 accepted-candidate planned transitions. The available inputs are Downloads (2), current Melodic (11), Electronic (22), MUG (29), plus restored DnB (14). The older Melodic recordings remain unavailable. All exposed sets are regression material, not untouched holdouts.

| Recomputed BEST list | Emergency handoffs, before → after | Zero-bar cuts, before → after | >3 dB loss, before → after | Maximum deficit, before → after |
| --- | ---: | ---: | ---: | ---: |
| Downloads | 0 → 0 | 1 → 1 | 0.85 → 0.85 s | 6.14 → 6.09 dB |
| Melodic | 0 → 0 | 2 → 2 | 1.30 → 2.35 s | 6.25 → 6.43 dB |
| Electronic | 0 → 0 | 7 → 7 | 22.75 → 3.75 s | 10.58 → 6.18 dB |
| MUG | 2 → 2 | 7 → 9 | 6.40 → 6.55 s | 15.55 → 15.04 dB |

Emergency means a failed plan or a one-bar fallback DryCut, matching the audit executable. The observer's generic `fallbacks` field also includes other fallback techniques and must not be substituted for this count. The lightweight residence script labels additional sub-two-bar fallbacks as emergency; use the definition above for this table.

All new cross-set plans have zero defined source, schedule and carried-state violations, and none of the current ORDER/new BEST foreground residences is below 30 source seconds. Musical regressions remain explicit:

- Melodic BEST now has two played-out departures versus zero before. Feel It Too → Emotional accounts for 1.45 s above 3 dB and 0.75 s above 6 dB. Keeping its previous BEST adjacency with the new fitter instead reduces total >3 dB exposure to 0.85 s. Thus this set demonstrates that a new globally ranked order can regress even when the local fitter improves.
- Electronic BEST reduces the large sustained deficit and retains zero emergency handoffs, but introduces a 0.30 s >3 dB voice-attenuation flag on Indigo Horizon → Spiritbond, a nine-bar vocal ride. This is an unresolved phrase/tail review signal. Current ORDER gains one zero-bar cut and its >3 dB exposure rises from 3.43 to 5.05 s; old BEST adjacency also worsens slightly, from 22.75 to 23.35 s.
- MUG retains Reborn → Alone: 5.75 s above 3 dB, 1.8 s above 6 dB, 1.05 s above 9 dB. Its short voice-attenuation flag on Dot to Dot → Your voice so... remains. More cuts are not counted as improved DJ flow.
- These four sets were re-planned and independently observed in this turn; their new full sequences were **not** rendered again. The real PCM acceptance above covers DnB only. Previous four-set renders document the earlier implementation, not these new orders.

The cross-set evidence does not support a claim of universally better BEST listening quality. Keep these unresolved musical gates separate from executable correctness and the two improved DnB adjacencies.

The accepted workspace suite passes **646 tests**, with **28 explicit ignores**, when run serially; the first parallel run encountered two existing native HID-discovery timeouts. Serial execution passed both without modifying controller code. The development bundle builds successfully. The independent observer passes **27 tests**. Planner tests pass **190 tests** with one explicit ignore, including the original counterexamples; the opt-in trace audit also compiles.

## Private artifacts and reproduction

Everything involving personal source data, full plans and generated PCM is under ignored `target/dnb-casefix-20261025/`. The accepted variant is named **`bars`** in that directory; earlier `loss`, `fit`, `sections`, `phrase`, `sustained`, `routed`, `rendered`, `final` and `guard` files are experiments, not acceptance evidence. In particular, the experiment labelled `final` predates the accepted local-bar window. Rejected experiments are retained so their failures are reviewable.

- `before.patch` and `source-before/`: exact starting worktree evidence.
- `planner-before`, `planner-bars`: frozen planner executables; unchanged frozen renderer is in the preceding DnB audit directory.
- `bars-report.json`, `bars-manifest-observer.json`: independent observer v3 with sensitivity results.
- `bars-*-plans.json`, `bars-*-metrics.json`, `bars-*-render.log`: carried-state plans, residence and engine assertions.
- `accepted-timeline.json`, `accepted-clips.json`, `accepted-silence-spans.json`: clip locations and continuous PCM scan.
- `regression-bars/`: cross-set fixed ORDER, old BEST adjacency and newly computed BEST plans.
- `source-check.json`, `accepted-manifest.json`: refreshed source checks and artifact/source fingerprints.

```sh
rtk proxy python3 target/dnb-casefix-20261025/run-candidate.py bars
rtk proxy python3 target/dnb-casefix-20261025/run-regression-bars.py
rtk proxy python3 scripts/evaluate-automix.py --manifest target/dnb-casefix-20261025/bars-manifest-observer.json --output target/dnb-casefix-20261025/bars-report.json --sensitivity
rtk proxy cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=1
rtk proxy ./dev.sh --build
```

`render-final.py bars` reproduces the three engine renders only at fresh output paths: the renderer refuses to overwrite existing WAVs. To enumerate feasible arrangements, use the ignored trace test with `MIXLESS_AUDIT_INPUT`, `MIXLESS_AUDIT_OUTPUT` and optional `AUDIT_PREVIOUS_PLAN`; the inputs contain local analysis exports, not production song exceptions. This task does not commit, push, install, restart or publish the application.
