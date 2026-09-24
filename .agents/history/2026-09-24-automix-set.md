# AutoMix set planning verification — 2026-09-24

## Behavior

- Automatic outgoing cuts and blend releases require a measured phrase boundary with change evidence, a section edge, or the actual recording end. A periodic bar marker with zero novelty does not authorize an exit. Explicit manual IN/OUT cues retain authority. Recovery and fallback paths use the same exit condition.
- Pair planning compares outgoing and incoming RMS relative to each recording’s active passages. A strong outgoing section cannot hand control to a weak fade-in merely because the incoming section is called an intro. Missing evidence does not manufacture a loudness guarantee.
- Whole-set planning carries actual entries, completed incoming windows, tempo and key forward. Published suffixes change together; unchanged evidence reuses the same plans. Separate laps do not create overlapping IN/OUT windows on one row.
- Playlist changes, reorders, download/basic-analysis/stem completion invalidate the relevant queue input. Decode, stem residency and dense automation compilation finish off-thread before replacing a staged candidate. Engine publication verifies the session, source generations, sample rate and remaining lead. An active or too-close handoff remains intact, and the new candidate gets the next slot even when preparation finishes after that handoff.
- Measured spectral ratios drive conservative cut-only correction: solo section correction is limited to 1.5 dB with a 0.15 dB/source-second curve slew, and overlap tonal correction to 2 dB with a smooth envelope. Existing bass-swap kills remain separate. Manual knob/kill interaction relinquishes the lane. Missing or quiet spectral evidence stays neutral.
- Subtle gestures use evidence-backed builds and low-vocal phrase boundaries; drum dips are at most 3 dB. The compact stems controls occupy a 76 × 76 logical-pixel square with four 36 × 36 cells.

No production branch matches a track title, ID or sample timestamp.

## Real-library evidence

Audits used a SQLite backup and the existing four-stem cache. Generated JSON, audio and logs stay under the ignored `target/automix-review/2026-09-24-final/` directory.

| Set | Tracks | Sequential handoffs | Result |
| --- | ---: | ---: | --- |
| Electronic | 20 | 19 | Complete, nonoverlapping windows, carried offsets, exact runtime plan reuse |
| DnB | 14 | 13 | Complete, nonoverlapping windows, carried offsets, exact runtime plan reuse |
| Melodic Dubstep | 11 | 10 | Complete, nonoverlapping windows, carried offsets, exact runtime plan reuse |

Electronic contains 10 phrase blends, one energy hold and eight structural cuts. Safe incompatible pairs still use structural cuts; the change does not force every pair into an overlap.

POWER-UP! → Don’t trust me now uses an eight-bar PhraseBlend: outgoing 92.759–107.306 seconds, incoming 13.605–26.407 seconds. The release at 107.306 seconds matches the measured section edge and a phrase boundary with confidence 0.691 and novelty 0.302. The earlier 101.851-second marker had zero novelty and no longer authorizes an automatic exit. These are audit observations, not planner constants.

Electronic was rendered continuously through all 19 plans using cached stem PCM: 2,582.12 seconds of output, maximum absolute sample 0.50912, no non-finite or out-of-range samples, and carried rate/pitch checks passed. This renderer checks transition execution; it does not execute desktop solo live moves or establish subjective listening quality.

The exported Subtle live-move audit contains 10 section-tone corrections, seven filter rises and six drum dips. Transition FX sends were zero for these Electronic plans. POWER-UP!’s solo drum dip is bounded to −3 dB and returns to neutral.

## Native application check

Built and launched `target/app/Mixless.app`, dev 0.1.0-beta.3, on macOS 27.0 (26A428), using the selected external-headphone device, master level 80%.

Observed through the native UI:

1. Electronic AUTO started Back Again and staged Still Here. List IN/OUT windows remained separated and future windows stayed stable while playback advanced.
2. Switching to Melodic while Back Again played replaced the silent candidate with Better Here With You; the outgoing transport continued. Switching back restored Still Here and the Electronic forecast.
3. Playback completed the first handoffs and reached POWER-UP!. Moving the following list entry ahead replaced the silent candidate with that entry. Restoring the original order replaced it with Don’t trust me and the actual POWER-UP! PhraseBlend began.
4. Both decks displayed the compact square stems controls without clipped labels.
5. Restored the original Electronic order and paused AUTO. The native application remains open with RESUME available.

The first launch attempt hit a native accessibility timeout together with delayed local commands; relaunching the same built binary restored normal operation. The cause was not established. Subsequent native playback and interactions above succeeded. No subjective audio acceptance or frame-time measurement is claimed.

## Automated checks

- `cargo test --locked --workspace --exclude mixless-desktop`: 379 passed, zero failed, 18 ignored by default.
- `cargo test --locked -p mixless-desktop`: 110 passed, zero failed, four ignored by default. The real-set ignored audit was separately run for all three sets above.
- `./dev.sh --build`: passed; latest dev bundle produced.
- Release serial callback budgets: audio, FX catalogue, recording and four-stem tests passed. The cue test initially reported 1.334 ms p99 against the test’s 1.333 ms target (128 frames; hardware block deadline 2.667 ms). Its isolated rerun passed at 0.911 ms p99 for 128 frames and 1.401 ms for 512 frames. The initial borderline failure is retained in `callback-budget.log`; the rerun is in `cue-budget-recheck.log`.
- `git diff --check`: passed. Changed Rust files formatted.

The focused regressions cover missing/low-confidence phrase evidence, tempo and recording-level transformations, energy continuity, EQ bounds and manual takeover, whole-set reuse/nonoverlap, safe playlist replacement, late reorder before/during a handoff, stale preparation, cancellation and preserving a deferred candidate when preparation outlives the current mix.

## Reproduce

Use an SQLite `.backup()` copy of the library. The ignored `audit_real_set_sequence` desktop test accepts absolute `MIXLESS_SET_AUDIT_DB`, `MIXLESS_SET_AUDIT_OUTPUT`, `MIXLESS_SET_AUDIT_PLAYLIST` and `MIXLESS_SET_AUDIT_STEMS` paths/values. It writes analyses, source paths, plans and Subtle move JSON. The `mixless-analyze` `render-set` example consumes those first three files plus model/cache directories and an output WAV path.

The offline result supports structural and execution correctness for these sets. Analysis boundaries remain estimates; listening acceptance still belongs to the user.
