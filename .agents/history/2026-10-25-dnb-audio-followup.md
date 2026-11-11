# DnB replacement-audio follow-up — 2026-10-25

The user restored the previously unavailable DnB recordings in `Downloads/DnB Set 260111`. This completes the current-audio DnB portion of the [six-playlist audit](2026-10-24-automix-full-audit.md). It does not establish by-ear acceptance or resolve the separately missing older Melodic recordings.

## Source and analysis checks

- All 14 M4A files are present and already imported as current playlist 7, track IDs 106–119. A new read-only SQLite backup was exported without `Library::open` reconciliation. No live database, playlist order or cue was changed.
- Every full-file BLAKE3 and canonical audio identity matches the current library's cached identity. Each analysis is version 26 and bound to the matching file hash. All 14 have version-2 stem evidence; frame coverage spans the complete source duration. Every stored stem frame/note matches the corresponding playback-cache manifest. Caches were copied into the audit directory before rendering.
- These are replacement encodings, not the missing original MP3 files. Thirteen durations differ by less than 0.1 seconds; Let me bloom is 211.39447 s rather than 204.14694 s, a 7.24753-second difference. Old source-coordinate plans were not reused on these files.
- The current playlist order differs from the old snapshot's order. Both were checked with the new recordings, and BEST was recomputed from the current list. No manual cues are set. No extra weighting or stem application was applied to already analyzed bars.
- Planner semantics remain ORDER_VERSION 8. This follow-up changes the independent Python observer, not the production planner, key-lock DSP or mixing envelopes.

Private artifacts are under ignored `target/dnb-audit-20261025/`: database backup, source identities, recording comparison, analysis exports, copied stem caches, frozen planner/renderer binaries, plans, reports, logs, complete WAVs and review clips. They are not committed recordings or library data.

## Planning results

Each run checks 15 handoffs: a 14-track circuit, its closing handoff and the repeated first departure. The planner carries actual entry, earliest next start, rate and pitch. The original order is mapped by the 14 unique titles to their new recording IDs; it is a diagnostic ordering comparison, not a controlled old/new detector comparison.

| New recordings in this order | Zero-bar cuts | Emergency handoffs | Overlapping schedules | Foreground residence below 30 source seconds | Minimum foreground residence |
| --- | ---: | ---: | ---: | ---: | ---: |
| Current ORDER | 5 | 2 | 0 | 0 | 44.31 s |
| Previous snapshot's ORDER | 6 | 3 | 0 | 0 | 68.36 s |
| Recomputed BEST | 3 | 1 | 0 | 0 | 59.22 s |

Emergency counts use the audit's terminal-fallback result, not every summary tagged `used_fallback`. Thirty seconds is an audit screen, not a musical minimum. The residence diagnostic uses control envelopes, not a complete perceptual model; loop-specific coordinates are covered by the engine/protocol tests and actual render.

Current ORDER's emergency edges are Lost & Found → Orbit and Orbit → Underwater. BEST retains Orbit → Feel This Good, exiting Orbit at source 235.91025 s of 237.30794 s and entering Feel This Good at 77.48286 s. Candidate traces reject alternatives for phrase/entry structure, vocal safety, harmonic carry and progression constraints. These counts are overlapping search rejections, not independent causes or proof that no better set exists. Do not remove safety gates to make the emergency count zero.

## Continuous playback results

Both current ORDER and BEST were rendered through the real engine with four-stem PCM, 48 kHz / 256-frame blocks, deck resets between uses and carried source/rate/pitch verification. Every handoff completed with signal, open incoming controls, finite samples and no clipping. This uses the corrected renderer from the prior audit.

| Mode | Duration | Handoffs | Peak magnitude | Near-zero spans of at least 100 ms |
| --- | ---: | ---: | ---: | --- |
| ORDER | 2525.91 s (42:05.91) | 15 | 0.55207 | Final source fade, 2523.94 s onward |
| BEST | 2197.22 s (36:37.22) | 15 | 0.71188 | Opening 0–0.22 s and final tail 2197.12 s onward |

Total rendered audio: 4723.13 seconds (78:43.13), 30 handoffs. The near-zero scan uses 20 ms bins and peak <1e-4; no interior run of five such bins was found. This does not prove absence of every shorter transient or perceptual gap.

The offline run does not reproduce the desktop's between-transition live moves or the user's customized master EQ/dynamics. It does not exercise the physical output device, user gestures or native UI. macOS remains 27.0.1 (26A434); no listening session is claimed.

## Independent observer defect found and repaired

Version 2 read an empty source curve as source second zero. Some legitimate cuts omit this curve and instead use source origin plus rate/clock; a long solo recovery can extend beyond an explicit source curve. The observer also inferred outgoing stop time from the source-curve endpoint rather than the scheduled overlap boundary.

This produced a false 117.74 dB peak-deficit warning for Divided Sky → Flicker, despite the rendered cut having continuous signal. Actual one-second output RMS around its first handoff stays approximately -22.7 to -20.1 dBFS; the repeated cut also has no near-zero gap. These measurements establish the warning was a source-coordinate error, not evidence of a 117 dB playback dropout.

Observer version 3 now:

- Integrates piecewise-linear rate against the device clock from the actual source origin and incoming launch bar when a curve is absent.
- Extends a short explicit curve through the remaining clock instead of reading a stationary sample.
- Stops the outgoing source at `clock(summary.length_bars)`, including zero-bar cuts, while retaining natural outgoing continuation only as counterfactual context.

Three regression tests cover explicit/implicit source equivalence, delayed entry with variable clock/rate and short-curve extension, and a zero-bar cut with recovery into a source whose opening is silent. All 27 observer tests pass. The version-2 report is retained as `observer-report-v2.json`; current conclusions use `observer-report.json` with `observer_version=3`.

The previous four available playlists were also re-evaluated against their unchanged frozen inputs. Their updated report is `target/automix-full-audit-20261024/observer-v3-report.json`. The prior main risk pairs remain; Electronic BEST's total exposure above 3 dB falls from 25.45 to 22.75 seconds. Version-2 measurements should not be mixed into new comparisons.

## Musical risks remaining

Version 3 reports no defined schedule/source/carried-state violations in the 45 DnB plans. It excludes one loop pair in current ORDER and one in BEST from unsupported signal diagnostics. Those are unavailable observations, not perfect scores. Timing agreement is conditional on the input grids; harmony and energy remain note/three-band proxies.

| BEST pair | Evidence | Optimization / acceptance question |
| --- | --- | --- |
| Lost & Found → Flicker | Predicted deficit >3 dB for 20.8 s, >6 dB for 1.25 s, max 7.65 dB | Check phrase ownership and routed melodic/drum levels through the long exchange. Compare against the natural outgoing passage before changing gain or fade duration. |
| Underwater → Orbit | Predicted max deficit 11.81 dB, >9 dB for 0.4 s; rendered output around the entry drops from roughly -27/-28 dBFS toward -32/-35 dBFS, then follows Orbit's quiet opening | Decide whether this section descent belongs in the set; evaluate a different incoming phrase or predecessor while preserving complete vocals and phrase boundaries. |
| Orbit → Feel This Good | Terminal emergency handoff remains; Orbit's final fade is followed by a louder incoming section | Explore feasible earlier phrase exits and bounded order alternatives before accepting a terminal cut. Current search failure does not prove musical impossibility. |
| Let me bloom → Divided Sky | Highest BEST tonal-roughness proxy, max 0.88; drum activity also changes | Audition the pitched stems together and compare rhythm density. A key label or phase-aligned beat grid does not establish compatible harmony or groove. |

BEST has fewer cuts/emergencies than current ORDER, but **greater modeled energy-loss exposure**: >3 dB for 23.3 seconds versus 0.15 seconds. The observer also finds two outgoing vocal-boundary crossings in BEST (Punching Holes → Take Control and the repeated Feel This Good → Loving, Loathing); neither crossing alone proves an audible syllable was truncated. No active voice attenuation above 3 dB was measured on the supported paths.

Do not label BEST an overall listening improvement from these counts. No threshold was tuned against this playlist to erase warnings. These recordings and their alternate encodings are exposed regression material, not untouched holdouts.

Three unmodified excerpts from the BEST output are available for review in the ignored audit directory:

- `lost-found-flicker.wav`, set time approximately 441.88–506.02 s.
- `underwater-orbit.wav`, approximately 1423.93–1449.45 s.
- `orbit-feel-this-good.wav`, approximately 1655.77–1680.12 s.

The excerpt timestamps are plan-derived and can differ slightly from cumulative device-block rounding. `audition-clips.json` records their exact extraction positions. Creating these clips is not listening acceptance.

## Reproduction

`run-plans.py` generates current ORDER, prior ORDER and BEST with frozen `planner`; `run-render.py` renders the current ORDER/BEST against isolated stem caches. `provenance-results.json`, `recording-comparison.json`, `best-diagnostics.log`, `best-timeline.json`, `pcm-silence-spans.json` and `final-manifest.json` retain the detailed evidence and fingerprints.

```sh
rtk proxy python3 -m unittest discover -s scripts/automix_eval -p 'test_*.py'
rtk proxy python3 scripts/evaluate-automix.py --manifest target/dnb-audit-20261025/observer-manifest.json --output target/dnb-audit-20261025/observer-report.json --sensitivity
rtk proxy cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=1
rtk proxy ./dev.sh --build
```

No production Rust audio change or ORDER_VERSION bump was needed for the observer repair. The workspace suite excluding desktop passed 643 tests with 28 explicit ignores and zero failures; the development bundle builds successfully. The observer's 27 tests also pass. Final outcomes are recorded in the audit logs and manifest. This task does not install or restart the running app, publish changes, or reopen the native screenshot acceptance from the previous audit.
