# Continuous percussion, arrangement contrast and heard overlap

The user reports that terminal short handoffs remain musically worse than earlier behavior, and asks that continuously drumming DnB be judged using other stems and the mixture contour. Earlier v19 vocal-association changes remain intact for later listening; lack of listening acceptance is not a rollback instruction.

## Retained mechanisms

`relative_recoveries` previously required a kit-level and attack retreat. It now also recognizes a two-sided, sustained non-percussive foreground retreat while a rhythmic bed remains. Both vocal-plus-instrument power and instrument mid/high power must retreat by at least 3 dB, beyond twice local variation, with a corroborating 1.5 dB mixture contour. Every candidate bar must support the retreat. Both neighbouring contexts, phrase-scale duration, local grid, coverage, actual return and buildup exclusions remain required. Instrument mid/high evidence prevents a bass-only mute from qualifying. Missing or nonfinite evidence adds no permission. These are conservative signal contrasts, not listening-calibrated probabilities.

This extends shared derived evidence; it does not relabel persisted sections or equate the window with a safe whole-track cut. Current grouped measurements contain vocals, drums and instruments; bass playback is separately controllable but does not have an independent fourth analysis meter. The detector uses the measured instrument spectrum and mixture RMS accordingly.

The composition fitter also had a duplicate instantaneous peak-power veto. Even a brief crossing below half outgoing power rejected an otherwise valid complementary stem exchange. The retained implementation judges sustained peak weight through the existing complete-local-bar check on final rendered routes in `foreground::preserved`. Its vocal, held-note, phrase, harmony and overall deficit checks remain. A regression distinguishes a quarter-bar dip from a sustained two-bar loss.

Overlap reward now uses the final crossfader, channel gain, EQ, filters, stem gains and actual source launch, through the same `experience` estimate used by final validation. The former theoretical pre-EQ occupancy could reward a nearly inaudible drum preload. Source reference powers are reused only while immutable tracks are borrowed for a fit. An optimistic bound uses the same source samples before EQ/filter attenuation to avoid rendering candidates that cannot win. It omits relative mix share, which attenuation can increase, and keeps a rounding margin. A 36-case routed-power test verifies the bound, and complete fixed/frozen corpus plans are compared with the unpruned implementation.

ORDER_VERSION is 20. Existing v26/stem-v2 payloads supply the observations; no formal library rewrite, analysis migration, audio-callback work, install, playback interruption, commit or publication is included.

## Evidence and limits

Live analysis identities, versions and payloads were checked against the consistent, full-source-hash-verified snapshot from the preceding investigation. All 76 recordings are development/regression material, not an untouched holdout. The new continuous-kit detector adds no relative windows in these current stored analyses: many obvious recoveries already have correct labels. This is not claimed as a correction to these recordings' structure. Synthetic counterexamples and gain, time offset, speed and half/double-clock checks exercise the new evidence path.

Attempts to broaden voice arrival, group rest samples, force earlier drum arrivals and expand late source entrances did not resolve the reported adjacent pairs. Their diagnostic source and plans remain in the private artifact directory and are not retained. Blindly widening the bounded entrance search displaced useful earlier entrances. No such search widening is in production.

The instantaneous-power correction opens an 8-bar Divided Sky → Flicker StemSwap with about 8.1 seconds of observed overlap, versus 4 bars and 2.5 seconds in the retained v19 baseline. That plan also includes a later solo pitch recovery; total plan duration must not be reported as transition overlap. The 56-bar Flicker → Lost & Found candidate has only about 7.1 seconds of scattered observed overlap and still leaves near the tail. It is explicitly not counted as a solved musical handoff. Five originally reported terminal short transitions remain in fixed DnB order: Orbit → Underwater, Underwater → Let me bloom, Punching Holes → Feel This Good, Feel This Good → In The Echo, and In The Echo → Power. Nominal bar counts are not acceptance.

Numeric flags, including possible lyric grouping and sustained-instrument loss, remain review evidence. No new listening acceptance was obtained. Final frozen-order, fresh-BEST, render, source hashes and test results are recorded in `target/automix-dnb-arrangement-20261028/final/`. Files at the parent artifact root are intermediate probes and must not be mistaken for final validation.

Final validation completed: all 12 comparisons and 11 offline renders; eight optimized fixed/frozen runs are identical to their unpruned plans. Final quiet workspace run: 705 passed, 28 ignored, 0 failed. Earlier intermittent native HID and acquisition timeout failures are retained in the artifact logs. The development bundle was rebuilt. No listening acceptance is claimed.
