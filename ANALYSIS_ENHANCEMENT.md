# Analysis enhancement

This note records the deployment decision for model assisted analysis on the macOS only v1 target.

## What runs today

The normal Rust analyzer remains the source of BPM, local tempo, downbeats, frequency bands, Chroma and structure. It computes multi scale 2/4/8 bar novelty directly from bar descriptors. Novelty peaks require sustained change and four-bar spacing; silence transitions are retained independently. This avoids constructing an O(N²) self similarity matrix and avoids mistaking one fill for a new section.

On macOS 12 or newer, the analyzer can call Apple's built in SoundAnalysis `SNClassifierIdentifierVersion1` classifier. It is a Core ML system model and does not add an ONNX Runtime or Python dependency. The bridge receives already decoded PCM, sends half second mono buffers with 1.5 second windows and zero overlap, and returns only exact human voice categories. It never runs on the audio callback, UI thread or mix planner.

The result is optional evidence. It can raise `BarFeature.vocal_presence` and sets `vocal_confidence` when a bar has at least 75% model coverage. It cannot lower the DSP risk, cannot claim stem isolation and cannot authorize a cue by itself. A bad callback, out of order interval, unavailable framework, concurrent job or budget miss leaves the DSP analysis unchanged.

## Bounds and measurements

`AnalysisOptions` defaults to a two second cooperative budget, one model job at a time and a ten minute track limit. The requested budget is clamped to five seconds and checked between half second chunks. It does not attempt to interrupt a Core ML operation already in progress. `native_vocals: false` selects the pure DSP path.

The release profile was measured on Apple M4, 16 GiB, macOS 27.0. Each mode ran in a separate process three times:

| Input | DSP features | model stage | process RSS, DSP → model |
| --- | ---: | ---: | ---: |
| 184 s local track | 141–149 ms | 327–445 ms | 167 → 195 MiB |
| 189 s local track | 143–146 ms | 334–347 ms | 171 → 199 MiB |
| 240 s synthetic drum track | 159–161 ms | 388–392 ms | 99 → 127 MiB |

The process RSS includes the decoded PCM and Rust runtime. The increase is the useful model comparison: roughly 28 MiB in these runs. System-service memory and actual Neural Engine dispatch were not measured. These numbers are a performance smoke test, not a recognition benchmark and not a guarantee for Intel Macs or older Apple Silicon.

Run the profile with:

```sh
cargo build --locked --release -p mixless-analyze --example analysis-profile
/usr/bin/time -l target/release/examples/analysis-profile track.wav
/usr/bin/time -l target/release/examples/analysis-profile --dsp-only track.wav
cargo test --locked -p mixless-analyze native_model_runs_and_deadline_discards_partial_results -- --ignored
```

## Why a custom structure network is deferred

Core ML can run custom models through the Neural Engine, GPU or CPU, and ONNX Runtime can target Core ML on macOS. Neither option is automatically cheap: a custom ONNX artifact needs a signed and versioned model, operator compatibility, runtime packaging, Core ML compilation cache handling and a CPU fallback. A model that is fast on the Neural Engine can still be slow during first compile or fall back to CPU for unsupported operators.

The next model may change a production cue only after a held out DJ set shows better phrase boundary F1 than the rule detector, p95 analysis time under two seconds for a four minute file, incremental RSS under 128 MiB, and zero realtime thread work. Until then the system vocal classifier is the safer enhancement; the structure model remains deterministic and auditable.



## Sources and next steps

- [Apple Sound Analysis](https://developer.apple.com/documentation/soundanalysis/classifying-sounds-in-an-audio-file) supplies the available system classifier; it is a general sound classifier, not a trained DJ section model.
- [Core ML](https://developer.apple.com/documentation/coreml) is the preferred deployment format for a future compact, fixed-shape music model. CPU fallback and actual operator placement need device measurement.
- [ONNX Runtime Core ML provider](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html) supports macOS 10.15+, with MLProgram requiring macOS 12+. Dynamic shapes and uncached model compilation can add cost; runtime support alone does not prove a specific model runs efficiently.
- [Foote novelty segmentation](https://www.audiolabs-erlangen.de/resources/MIR/FMP/C4/C4S4_NoveltySegmentation.html) informs the current structural change detector.

The most useful next dataset is annotated transitions: section boundaries, completed vocal phrases, safe cue windows, and beat/downbeat corrections. Evaluate boundaries within both 0.5 s and one beat, and report mid-phrase cuts and double-vocal overlaps separately. A small CNN/TCN on shared spectral features is a candidate for training or distillation; no custom weights or accuracy claims are included in this change. Validate on M1/M2 and Intel before enabling any more expensive model by default. The budgets above are proposed acceptance targets, not measured p95 or accuracy results.

## Validation on 2026-09-06

`cargo test --locked --workspace --exclude mixless-desktop`: 135 passed, 8 intentionally ignored. The macOS system-model test was also run explicitly and requires actual inference results, followed by a forced deadline failure that leaves DSP data intact. Its inference smoke pass uses a five-second allowance to separate cold model startup from the default two-second production budget. Cold startup under compilation load did exceed the default budget in one run and correctly fell back.

`cargo check --locked -p mixless-desktop` and `./dev.sh --build` passed, including native framework linking. Changed Rust files passed rustfmt checking. No realtime device listening or labelled music accuracy benchmark was performed.
