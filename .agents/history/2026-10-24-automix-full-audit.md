# AutoMix execution, display and all-playlist audit — 2026-10-24

This follows the [foreground-continuity investigation](2026-10-24-set-foreground-continuity.md). The user reported harmony and energy discontinuities, incomplete stem phrases, short track residence, and disagreement between the toolbar, library IN/OUT markers and playback. This record separates code correctness, planning, rendered audio and listening acceptance. No by-ear acceptance is claimed.

**2026-10-25 follow-up:** [DnB replacement-audio validation](2026-10-25-dnb-audio-followup.md) now covers its 14 newly downloaded recordings and repairs the independent observer's absent-source-curve handling. The four available lists below were re-evaluated with observer v3; use `observer-v3-report.json` for current diagnostics. The v2 artifact is historical. The unavailable-original counts below describe the 2026-10-24 snapshot.

## Scope and provenance

The review followed analysis/cues, playlist preparation and cache invalidation, BEST ordering and carried entry/rate/pitch, host staging and cancellation, compiled automation, callback execution, and desktop projections. Existing tests exercise missing stems, manual cues, late start, EOF, cancellation, repeated entries, playlist changes and both deck directions. This is a broad review and regression run, not proof that every possible AutoMix state is bug-free.

The live SQLite database was backed up without opening the backup through `Library::open`: opening a library may reconcile folder membership, which is inappropriate for a frozen corpus. The initial mutable-copy/alias experiment is excluded from final comparisons. Original playlist occurrence order and manual cue state were exported; none of these six playlists had manual cues.

- 89 original library entries: 64 available recordings have freshly verified canonical audio identities matching their stored hashes; 25 originals are missing, with no changed available recordings.
- The live snapshot had 62 analyses at version 26 and 27 at version 23. The two available Downloads recordings were reanalyzed in an isolated copy; the four available playlists therefore use version 26 throughout. The live library was not reanalyzed or modified by this audit.
- Two folders have the same Melodic playlist name. Playlist 3 contains the 11 available recordings (IDs 95–105); playlist 6 contains the 11 unavailable older entries (IDs 17–27). Seven old/new identities match, but four do not; these were not silently substituted for the original missing corpus.
- The frozen working-tree baseline is ORDER_VERSION 7, including the earlier musical fixes. This audit uses ORDER_VERSION 8. It is not a comparison with the previously released app or with the user's original listening session.
- Personal database copies, paths, analysis payloads, stem caches and rendered recordings remain under ignored `target/automix-full-audit-20261024/`. They are not repository artifacts.

## Confirmed defects and changes

### Exact executing plan and honest live status

The queued desktop `Plan` message could lag a same-pair replacement and combine an old plan with current progress. The engine now publishes a monotonically increasing plan identity after resetting its clock/flags. Snapshots expose that identity; desktop retrieves exactly that immutable host-owned plan. A snapshot crossing publication invalidates its identity rather than pairing inconsistent metadata. Track identities are checked in both deck directions before showing the transition.

The toolbar previously inferred the technique from instantaneous stem controls. A StemSwap waiting for its incoming layer could therefore be relabeled PhraseBlend. The technique now describes the published complete plan; the live status separately reports scheduled, waiting, finishing, paused or manual operation. Closed-channel checks include playback, fader, trim, crossfader/reverse and all stem gains. Send FX labels use the actual planned kind rather than always claiming Echo.

Manual lane/gain takeover is exposed in the snapshot. The toolbar shows Manual; deck/library windows and tooltip ranges/stages are hidden because they no longer describe all executing controls. Stale tooltips disappear when the plan ends or identities no longer match. Source-clock-changing BeatJump, reverse, jog touch and jog commands now cancel the schedule, as seek/cue already did, instead of continuing an invalid source-time plan.

### Shared source-time windows and residence

The old displayed range used the full scheduled interval, including silent preparation and solo recovery. `MixPlan::mix_windows` now derives a shared dry exchange window from gain, equal-power crossfader and independent stem envelopes. Toolbar, deck and library consume the same source-coordinate calculation. Zero-length cuts remain point markers; loop windows show the repeated source region. Additive, serde-default source sample rates preserve older serialized plans.

These windows describe control envelopes, not measured audio audibility. EQ/filter attenuation, an already silent source passage and FX tails can still change the audible impression. The display does not claim otherwise.

Some fallback plans have no explicit source warp curve. Sampling that empty curve returned zero and corrupted established-entry/foreground residence costs. `source_seconds_at` uses source origin plus the integrated rate/clock in that case, handles loop wrapping, and extends a short explicit curve through the remaining clock. The planner now uses that shared mapping. Tests include a nonzero 120-second fallback origin, incoming origin/rate, variable clock and loops. ORDER_VERSION 8 invalidates persisted results calculated with the older semantics.

Library forecasts are filtered against current adjacency even when an active pair exists. Repeated occurrences are matched by sequence position; an ambiguous active occurrence is hidden instead of painting every copy. Windows are shown only for the active AutoMix playlist, and stale neighbors disappear after reorder/stop/manual takeover.

### Continuous renderer correction

**The previous continuous-render evidence was invalid.** `render-set` reused the outgoing deck without restoring its channel fader. Desktop `reset_deck` already restores it. The harness could complete source clocks while producing silence after early handoffs. The previous `target/set-regression-20261024/validated-continuous.wav` was verified silent at 500, 1000 and 2000 seconds. Its duration and peak cannot establish continuous audible playback. The old history entry is corrected explicitly.

The renderer now restores deck controls before reuse, matching desktop staging, and rejects a handoff without at least 0.1 seconds of signal above 1e-4 or with the incoming fader/stems left closed. Source/rate/pitch endpoint checks remain. The early `melodic-best-full.wav` and `mug-best-full.wav` from this audit are also superseded; only `final-*-continuous.wav` is valid final evidence. Pair renders are a separate harness and are not invalidated by this finding.

## All-playlist results

Each mode checks one full circuit, the last-to-first handoff and the next departure: N+1 pairs. Both fixed ORDER and recomputed BEST were run against identical inputs with carried source entry, rate, pitch and earliest next start. There are 190 final pair evaluations across 12 runs.

| Playlist | Entries | Evidence | Pairs per mode | ORDER zero-bar cuts / emergency handoffs | BEST zero-bar cuts / emergency handoffs |
| --- | ---: | --- | ---: | ---: | ---: |
| Downloads (1) | 2 | Available audio, v26 | 3 | 1 / 0 | 1 / 0 |
| DnB (2) | 14 | Missing originals, cached v23 only | 15 | 7 / 2 | 3 / 0 |
| Melodic (3) | 11 | Available audio, v26 | 12 | 5 / 3 | 2 / 0 |
| Electronic (4) | 22 | Available audio, v26 | 23 | 10 / 4 | 7 / 0 |
| MUG (5) | 29 | Available audio, v26 | 30 | 12 / 7 | 7 / 2 |
| Older Melodic (6) | 11 | Missing originals, cached v23 only | 12 | 2 / 1 | 3 / 0 |

Emergency counts use the audit pair result's `used_fallback`, which denotes the terminal fallback path. Raw summary `used_fallback` also labels some deliberate short paths and is not interchangeable. Zero-bar cuts exclude the emergency one-bar preparation-and-cut plans. Neither count by itself measures naturalness.

The final numerical observer found no overlapping schedules and no foreground residence below 30 source seconds in these 190 plans. Thirty seconds is an audit screen, not a universal musical acceptance threshold. Envelope residence ignores some filter/EQ effects; the Python diagnostic does not reconstruct loop wrapping, so loop-specific source correctness relies on protocol/engine tests and the renderer.

Compared with the frozen v7 baseline, **all six playlist orders and all executing curves/times are unchanged**. Two BEST LoopOut summary scores change because their source-clock residence is now calculated correctly. Thus this round demonstrates display/execution fixes and no new scheduling changes on this corpus; it does not demonstrate a new whole-library listening improvement. The earlier phrase/energy changes remain in both sides of this comparison.

### Actual continuous four-stem render

The four available BEST lists were rendered through the real engine at 48 kHz / 256 frames, including loop closure and the repeated first departure. Each reused deck is reset; all handoffs have signal, incoming controls open, finite samples, no clipping and matching carried source/rate/pitch endpoints (0.03 seconds / 0.002 rate / 0.02 semitone tolerances).

| Playlist | Duration | Handoffs | Peak magnitude | Whole-second bins below 1e-4 peak |
| --- | ---: | ---: | ---: | ---: |
| Downloads | 367.36 s | 3 | 0.52439 | 0 |
| Melodic | 1512.25 s | 12 | 0.67186 | 0 |
| Electronic | 2570.13 s | 23 | 0.58269 | 0 |
| MUG | 3454.77 s | 30 | 0.62085 | 4 |

Total: 7904.51 seconds (2 h 11 min 44.51 s), 68 handoffs. MUG's near-zero bins are its opening second, second 2191 during blue comet's solo before the next transition, and the last two seconds of the final source tail. The solo bin is not a closed reused fader; it remains an explicit source-passage review point. This check does not rule out shorter dropouts or establish subjective energy continuity.

The harness does not reproduce the desktop's between-transition live moves, physical device output, controller interaction or all concurrent library operations. These are offline stem PCM results, not a physical listening session.

## Remaining musical review points

An independent observer evaluated the four available lists in both modes with threshold sensitivity. It reports zero defined invariant violations, but four run-pair occurrences use unsupported loop processing and must not be scored as perfect. Detected vocal activity crossing a boundary is a review signal, not proof that an audible syllable was cut. Chroma roughness, three-band energy and source grids are proxies, not human harmony/phrase labels.

| BEST pair | Evidence | Next acceptance criterion |
| --- | --- | --- |
| blue comet → Brain Power; Luolimasi → Still in my heart | Two MUG terminal emergency handoffs remain | Review whether the bounded order search can avoid these edges while retaining phrase, vocal and tempo constraints; do not remove safety checks to obtain zero fallbacks |
| Reborn → Alone | Predicted peak deficit max 15.55 dB, >9 dB for 1.1 s; rendered output falls locally near handoff before incoming arrival | Listen in context against the natural outgoing continuation; distinguish a deliberate break from excessive combined attenuation |
| Back Again → Still Here | Predicted peak deficit max 10.58 dB; Electronic BEST has 22.75 s total >3 dB across pairs after observer v3 correction (25.45 s in v2) | Inspect per-pair rendered energy contours and arrangement ownership, not only set-average loudness |
| Found You → Ice Shards; Take You Home → Stay With Me; Particle Arts → The Spark | High tonal-roughness exposure remains | Audition pitched stems separately and together; local key compatibility alone cannot establish chord/melody compatibility |
| Dot to Dot → Your voice so... | Predicted vocal attenuation reaches 3.36 dB for a very short exposure (>3 dB: 0.05 s) | Check the sung phrase and routed PCM before treating this proxy as a cut vocal |

These exposed recordings are regression material, not untouched holdouts. No new threshold was tuned to erase these warnings. A future musical change should use complete phrase annotations, hard counterexamples and additional independent recordings, then repeat fixed-order and BEST comparisons. The missing 25 originals prevent current-audio validation of DnB and the older Melodic playlist.

Key lock preserves pitch while speed changes; it does not harmonize two songs. AutoMix separately selects pitch offsets from local tonal evidence and evaluates the overlapping material. The earlier v26 tonal/confidence repair and engine pitch tests remain relevant; none of these checks promises that every sustained synth chord or vocal overlap is consonant.

## Validation and reproduction

- Workspace excluding desktop: 632 passed, 0 failed, 28 explicitly ignored, serial run.
- Engine + protocol after final publication changes: 182 passed, 0 failed, 9 ignored. Protocol includes four source-window/clock regressions.
- Desktop full suite: 208 passed, 0 failed, 7 ignored at that working-tree snapshot. Final tooltip/manual-state adjustment: all 9 `views::automix` tests passed. Other concurrent working-tree changes are outside this record.
- Isolated serial release callback budgets: all 5 passed. Dual-deck stems + pitch + FX + recording + cues p99 0.959 ms, max 1.143 ms against a 2.667 ms callback. Ordinary 48 kHz / 256-frame audio p99 0.197 ms against 5.333 ms. This is a local benchmark, not a physical-device stability or animation measurement.
- Development bundle builds with `./dev.sh --build`. No installation, restart of the user's running app, commit, push or release was requested or performed.
- Environment: macOS 27.0.1 (26A434). System default output was external headphones; this audit did not play through them. Native preview uses a separate app ID and temporary offline profile. Native capture status is recorded separately below; unit layout tests are not a replacement for a screenshot.

```sh
rtk proxy cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=1
rtk proxy cargo test --locked -p mixless-desktop -- --test-threads=1
rtk proxy cargo test --locked --release -p mixless-engine callback_budget -- --ignored --nocapture --test-threads=1
rtk proxy ./dev.sh --build
```

The ignored audit directory contains `source-before/`, `before.patch`, frozen `planner-final` and `renderer-final`, original `source-library.db`, `set-{id}.json`, `loop-{id}.json`, `baseline-*`, `final-*`, `provenance-results.json`, `observer-final-manifest.json`, `observer-final-report.json`, test/build logs and final rendered WAVs. `final-manifest.json` records source/binary/input/output fingerprints. The observer can be reproduced with `scripts/evaluate-automix.py` and its saved manifest; it does not use the planner's own score as its quality metric.

### Native visual acceptance still open

The [mixless brand skill](../skills/mixless-brand/SKILL.md) requires: “Capture the current built native app at Retina resolution after visible UI changes.” The separate offline preview launches, but the computer-use tool returns `cgWindowNotFound` for its bundle ID and app path, including after a fresh rebuild and a bundle-name correction. No final current screenshot was obtained. The earlier `native-1180.png` used a faulty fixture that displayed Manual for every row and is excluded. No accepted capture was added to `assets/screenshots/capture.json` by this audit. Automated toolbar-width, tooltip wrapping and lifecycle tests pass, but final native visual inspection remains outstanding; do not label it complete from those tests.
