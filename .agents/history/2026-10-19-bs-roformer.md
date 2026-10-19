# Single-model four-stem BS-RoFormer (2026-10-19)

The user replaced the earlier FT selection with a single model producing four stems, to favor shared predictions during remix playback. This is a model choice, not a claim that independent FT predictions necessarily sound wrong or that a joint model enforces mixture consistency.

## Implementation

- Pin the Silverdaw host-STFT export of MSST v1.0.12 `model_bs_roformer_ep_17_sdr_9.6568`, HF revision `9fa8c3acdbd488b97309e4b78c9c605389cec777`, artifact SHA-256 `4bac38e42c085b5d69964e2b7ff161318a1315122c10b025c718b20e94d85d6b`, 269,457,840 bytes.
- One graph call per 8-second chunk outputs drums, bass, other and vocals together. Native Rust performs periodic-Hann, centered/reflected STFT (2048 FFT, hop 441), iSTFT with window-square normalization, 50% chunk overlap, endpoint fades and border reflection/cropping. No independent stem normalization.
- Model workers own both real and imaginary tensors. Recovery reuses both inputs; output shape and finite checks cover both four-stem spectra. The normal provider chain is MLX → CPU. Explicit CoreML is rejected for this graph and falls back to CPU.
- The first CoreML diagnostic reached more than 10 GiB RSS during compilation, then the process received SIGKILL. The known orphaned test compile cache (2,049,319,672 bytes) was removed under its lock. Native provider verification was rerun successfully with the intended MLX/CPU policy.
- Analysis version 23 refreshes the mixture-derived features before new stem evidence is applied; model hash separates BS caches from FT caches. Full-file source/package hashes and audio-stream stem identities remain separate.
- Keep the production residual mixing formula: unity is original PCM, and Other includes the separation residual when changing gains. All stems share one source clock and stretcher. Callback/DSP code was not changed.

## Evidence

- macOS 27.0.1, Apple Silicon, native ORT MLX/CPU. No system audio or subjective listening claim.
- Same 15-second interval used for the previous FT diagnostic: load 9.89 s, separation 6.25 s, notes 0.14 s, residual RMS 0.009989. Prior FT was 13.65 s separation, residual RMS 0.019726. Actual chunk policies differ; this is not a generalized quality/performance ranking.
- Rust versus torch 2.12.1 STFT max absolute difference: 3.0518e-5. Native iSTFT on the torch spectrum reconstructs source PCM within 2.9803e-7.
- Native CPU versus MLX spectra max absolute difference: 4.9591e-5. Cancellation and injected provider recovery pass with both spectral inputs preserved; CPU and one-input Basic Pitch remain usable.
- Real BS predictions rendered through the production offline engine: unity is bit-identical to the dry path; individual vocals/drums/bass/Other mutes and half-vocal gain are finite and change the expected output; all-muted output settles to zero after the existing 150 ms gain smoothing. WAV review artifacts are under ignored `target/verification/bs-roformer/playback`.
- Full-track generation completed for two current sources (285.21 and 204.15 s) in 68.3 and 43.2 s, respectively. Both cached results subsequently loaded through the production playback/resampling path at 44.1 kHz with independent bass present. Current model caches use audio-stream hashes in the live stem-cache directory; personal audio/analysis inputs remain ignored.
- Removed the retired FT cache entries (700,628,287 bytes) and four checksum-verified FT weights (662,450,544 bytes) under the application locks. Retained both current BS caches, BS weights, Basic Pitch and the original source/DB recovery backup. Repeated metadata maintenance wrote zero tags and reported zero errors.
- Bundled/offline native test: analyzes new audio without a download directory, then actually executes both model sessions from the read-only-style model source. Passed with MLX+CPU.
- Workspace excluding desktop: 584 passed, 28 ignored; desktop: 191 passed, 7 ignored. The first concurrent workspace doctest attempt lost an rlib while another Cargo build refreshed the shared target; a sequential complete rerun passed. Content validation: 24 pages, 134 search records, zero errors/warnings. Development bundle build passed.
- Final stem unit rerun: 20 passed, 6 ignored. Final development bundle is `target/app/Mixless.app`, bundle ID `app.mixless.desktop`, version `0.1.0-beta.4` build 1, executable SHA-256 `ef55dcf3d0f91a69a4ed60788cf601a089187277c91dfeca5d68afe9fc231120`. Both upstream code license files are present in `Contents/Resources/Licenses/`. No installation, commit or release was requested or performed.

The export advertises MIT, and the two upstream code licenses are retained in `third-party/`. An explicit grant for redistributing the original checkpoint remains unresolved in [MSST issue 249](https://github.com/ZFTurbo/Music-Source-Separation-Training/issues/249). No model mirror upload, public release, legal clearance, physical Traktor reimport or listening comparison is claimed.
