# Stem processing verification

The storage setting defaults to 32-bit float WAV. Newly started analysis jobs
snapshot the setting before separation; cached entries retain their format.
The FLAC option uses libFLAC 1.5.0 at compression level 8, converting floats to
24-bit integer PCM. MP3 uses LAME 3.100 at 320 kbps, quality 0 and 44.1 kHz stereo.
Power-of-two scaling preserves separator headroom; the manifest records the
restoration gain. FLAC is lossless for the converted PCM, not bit-exact for the
original floats. Musical evidence and notes are computed before compression.

Encoders run on background workers. FLAC converts in bounded blocks and verifies
its own output. Before publishing any cache, mixless decodes each file completely
and checks exact frame count, channels, sample rate and finite samples. The
manifest records file lengths and BLAKE3 hashes. Playback checks integrity and
uses strict gapless decoding, then resamples into the existing StemBuffer. No
codec, file operation or new lock runs in the audio callback. Unity gains keep
the original-track path; isolated/subtracted MP3 stems remain perceptually lossy.

Portable cache names come from a closed format enum, never paths in imported
metadata. Old float WAV manifests remain supported. A compressed package requires
a receiving version with compressed-cache support; older versions do not gain
codec support merely by receiving a new package.

## Regression coverage

- Exact frame counts, distinct stereo channels, headroom restoration and impulse
  positions near the start, middle and end, including MP3 gapless delay/padding.
- Very short files, silent files, complete FLAC blocks, short final blocks, high
  amplitude noise, quantization bounds, corrupt payloads and cancellation.
- Complete cache publication, 44.1/48 kHz playback, portable installation, reuse
  after changing the preferred format, legacy WAV and invalid storage metadata.
- The actual offline engine at 48 kHz output with both 44.1/48 kHz sources:
  unity against the original, isolated vocals against the decoded reference,
  tempo/pitch, cue jumps, loops, reverse and reverb. Gain ramps settle while
  paused before comparing stretcher state; renders use 128-frame blocks.
- FLAC and MP3 .mixpack export/import/re-export, exact loaded length and retained
  encoding. The small committed fixtures are synthetic, not acquired music.
- Actual GPUI controls at 940 x 820 and 720 x 540, all three selections, saved
  settings, live processor configuration, failed persistence and reset defaults.

## Real audio audit

The ignored `audit_real_caches` test reads existing user caches without changing
them, chooses three full tracks by duration and encodes all four lanes through
the production save/load paths. It deletes its output audio after measurement.
No acquired audio is included in this repository. Measurements and build logs
are under ignored `target/verification/stem-storage/`.

The initial encoder candidate produced an invalid FLAC frame on a complete
330.095-second instrument lane, independently reproduced in FFmpeg. It was
replaced by libFLAC; the production prepublication decoder rejected that file,
so no damaged cache was installed. The final implementation is validated against
that complete source as well as the synthetic block-boundary cases.

These checks establish codec integrity, timing and offline DSP behavior. They do
not establish subjective listening quality, physical audio-device behavior,
Intel hardware performance or release signing.

Measured on macOS 27.0.1 (26A434), Apple M5 Max, 64 GiB RAM, development
build with optimized codec dependencies. Timings are local wall-clock
observations, not release performance guarantees. No audio output device was
used; this is decoding and offline rendering.

| Source duration | Format | Audio bytes / float PCM | Encode and verify | Load three playback lanes |
| --- | --- | --- | --- | --- |
| 127.501 s | WAV | 100.00% | 2.00 s | 1.34 s |
| 127.501 s | FLAC | 43.32% | 2.85 s | 0.91 s |
| 127.501 s | MP3 | 11.34% | 14.89 s | 0.39 s |
| 220.148 s | WAV | 100.00% | 3.25 s | 2.24 s |
| 220.148 s | FLAC | 46.87% | 5.04 s | 1.64 s |
| 220.148 s | MP3 | 11.34% | 28.95 s | 0.67 s |
| 330.095 s | WAV | 100.00% | 4.93 s | 3.39 s |
| 330.095 s | FLAC | 46.20% | 7.35 s | 2.43 s |
| 330.095 s | MP3 | 11.34% | 39.52 s | 0.99 s |

All 36 encoded lanes retained their complete sample count. WAV matched exactly.
The largest FLAC error was 1.1920929e-7 with a restoration gain of 2 (half a
24-bit step after restoring headroom). MP3 is lossy; no perceptual-equivalence
claim is made.

## Final local checks

- `cargo test --locked --workspace --exclude mixless-desktop`: 475 passed,
  26 ignored. Includes 18 stems tests and 10 package tests.
- `cargo test --locked -p mixless-desktop`: 150 passed, 7 ignored.
- `audit_real_caches --ignored --nocapture --test-threads=1`: passed all nine
  whole-track format cases, including the formerly failing instrument lane.
- `./dev.sh --build`: passed, zero warnings; the development app bundle includes
  the new codec license notices.
- Site `check:content`, `check` and `build`: passed.
- FFmpeg independently decoded the final generated FLAC and MP3 package fixtures
  to exactly 4,410 frames without errors. Earlier non-block-aligned impulse
  validation also established MP3 320 kbps, stereo and exact transient positions.
- Native UI review selected/saved FLAC and MP3 at 940 x 820, reopened the saved
  setting at 720 x 540, scrolled and switched to FLAC with its precision note
  fully visible. See the Preferences verification record and original captures.
- x86_64 `mixless-stems` and `mixless-pack` tests: 19 passed, 1 ignored under
  Rosetta, including all codec tests and the compressed package round trip.
  This used the installed rustup stable compiler/target (Rust 1.99.0) and
  `DYLD_FALLBACK_LIBRARY_PATH=/usr/lib/swift` for the macOS test harness.
  It is x86_64 runtime evidence under translation, not physical Intel hardware.

## Resource-aware separation concurrency

The recording pool now scales from one to four independent model sessions using
both logical CPU count and physical RAM. Additional resident model sets are
budgeted at 6 GiB each after retaining CPU and RAM headroom; every recording also
passes a live admission check before model loading or full-track PCM allocation.
On macOS this uses free plus inactive VM pages and memory-pressure state, without
double-counting speculative or purgeable pages. Reservations include estimated
future model/activation, decoded audio and cache-verification memory.

Warning pressure admits at most one new recording; critical pressure starts none.
Missing resource information falls back to one. Running native calls finish
normally. Waiting tasks are cancellable, release idle models and show a waiting
status. A recording whose estimate cannot fit the machine fails clearly rather
than waiting forever. Model loading and predictions share the existing process
CPU budget with basic analysis. Session creation/execution/destruction remain on
their dedicated utility-QoS threads; content locks and atomic cache publication
are unchanged.

Local native validation used four distinct synthetic 16-second recordings on
macOS 27.0.1, 18 logical CPUs and 64 GiB RAM. The old two-session pool completed
the batch in 35.42 s (peak footprint 9.29 GiB); the new pool reached four active
recordings and completed in 17.20 s (peak footprint 16.73 GiB). These are single
local wall-clock observations with other apps running, not a general speedup
guarantee. All 16 WAV lanes and all four analysis/notes payloads were identical.

A separate paced offline playback run used cached separated stems, a loop and
two-semitone pitch shift while new native inference ran. Resource admission
limited that run to three active recordings and held one waiting until memory
was available. After 128 initial warmup blocks, all 6,846 measured 128-frame
callbacks at 48 kHz finished below 2.667 ms (p99 0.138 ms, max 0.262 ms), with
finite bounded PCM. An earlier un-warmed diagnostic included a 132 ms startup
outlier; the final run describes an already running deck. This is offline
evidence, not physical-device or subjective listening validation. Estimates
are admission reserves, not hard allocator limits or GPU preemption.

Artifacts and comparisons are under ignored `target/stem-concurrency-20261024/`.
The `parallel-benchmark` example exercises production admission and cache paths;
`MIXLESS_BENCH_PLAYBACK_CACHE` enables its paced playback diagnostic.
