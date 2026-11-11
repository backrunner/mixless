# Resource-aware stem concurrency

The existing two-session pool was already concurrent, but its ceiling depended
only on CPU count. Pool sizing now considers physical RAM and uses up to four
independent model sets. On an 18-core policy fixture, 8/16/32/64 GiB select
1/1/3/4 candidate sessions. Two logical CPUs remain outside the shared background
CPU budget when available. Inference thread counts scale down as the session
count increases, retaining the basic-analysis share of the same budget.

Per-recording admission samples macOS free/inactive VM pages and memory pressure
before allocating decoded PCM or loading models. A mutex/condition-variable budget
reserves estimated future memory, not only allocations visible at that instant.
Warning pressure admits at most one recording, critical pressure starts none,
and unknown measurements fall back to one. In-flight native calls finish normally.
Waiting tasks remain cancellable, report waiting for resources and release idle
model sessions. A recording whose estimate cannot fit the machine fails explicitly
instead of waiting indefinitely. Model initialization now also holds its CPU share.

Dedicated model threads, provider fallback, source identity, content locks, cache
publication and algorithms are unchanged. No audio callback code was modified.
No source audio, live library, current stem cache or installed weights were changed.
All generated synthetic audio/cache material remains under ignored target output.

## Native measurements

macOS 27.0.1, 18 logical CPUs, 64 GiB. Checksummed installed BS-RoFormer and
Basic Pitch weights, automatic native backend policy, four distinct deterministic
16-second stereo recordings at 44.1 kHz. Timings include loading, separation,
transcription, cache publication and playback-cache validation. Other apps remained
running; these single-pass wall-clock measurements are not general speedup claims.

| Pool | Peak active recordings | Batch time | Peak process footprint |
| --- | --- | --- | --- |
| Original two sessions | 2 | 35.416 s | 9.29 GiB |
| Resource-aware four-session pool | 4 | 17.204 s | 16.73 GiB |

All 16 generated WAV lanes were byte-identical between both versions. All four
analysis/notes payloads were identical. Existing cache/model versions remain valid.

The final paced offline playback diagnostic used an existing stem cache, a
four-beat loop and two-semitone pitch shift alongside new native inference. The
live memory gate admitted three recordings and held the fourth waiting until
13.49 s. That batch finished in 18.254 s, with peak footprint 13.08 GiB. After
128 warmup blocks, all 6,846 measured 128-frame callbacks at 48 kHz finished below
their 2.667 ms period: p99 0.138417 ms, max 0.261916 ms, zero over-deadline blocks.
Samples were finite/bounded and the deck remained playing with stems attached.
The resulting 16 WAV lanes and analysis payloads also matched the baseline exactly.

An earlier non-warmed diagnostic included a 132.48 ms startup outlier. The final
diagnostic measures an already running deck, not initial stretcher setup. No
physical soundcard, live-device underrun, GPUI frame-time or listening acceptance
is claimed. Admission estimates are not allocator quotas or GPU kernel preemption.

## Validation

- Final full workspace excluding desktop, serial: 625 passed, 28 ignored.
- Full desktop suite: 198 passed, 7 ignored.
- Focused protocol/stems tests: 42 passed, 6 ignored, including CPU/RAM sizing,
  memory pressure, unknown samples, recovery, cancellation and impossible jobs.
- `./dev.sh --build`: passed, refreshed the development app bundle.
- `git diff --check`: passed. Existing unrelated changes were preserved.
- The currently running app/playback was not restarted.

Raw logs, binary/model hashes and comparisons:
`target/stem-concurrency-20261024/`. `manifest.json`, `comparison.json` and
`final-comparison.json` fingerprint the run and its outputs. The reproducible
entry is:

```sh
rtk cargo run --release --locked -p mixless-stems --example parallel-benchmark -- MODEL_DIR OUTPUT_DIR 4 16
```

`MIXLESS_BENCH_PLAYBACK_CACHE` enables the paced playback diagnostic using an
existing benchmark cache directory.
