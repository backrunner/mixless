# Stem model comparison (2026-10-19)

For mixless, the required outputs are drums, bass, other and vocals. An architecture name is not a checkpoint identity. All values below are published upstream results, not local listening or a new cross-model benchmark.

## Comparable published scores

The following rows use the Multisong SDR measurements published by [MSST](https://github.com/ZFTurbo/Music-Source-Separation-Training/blob/main/docs/pretrained_models.md). Higher is better; scores are dB. The FT average is the arithmetic mean of MSST's four specialist scores.

| Checkpoint | Average | Vocals | Drums | Bass | Other |
| --- | ---: | ---: | ---: | ---: | ---: |
| Standard HTDemucs, 955717e8 | 9.16 | 8.24 | 10.88 | 11.76 | 5.74 |
| HTDemucs FT, four official specialists | 9.33 | 8.38 | 11.13 | 11.96 | 5.85 |
| SCNet-Large, starrytong fixed checkpoint (MSST v1.0.9) | 9.28 | 9.05 | 11.23 | 11.27 | 5.57 |
| BS-RoFormer 4stem, ep17 (MSST v1.0.12) | 9.38 | 9.19 | 11.29 | 11.08 | 5.96 |

FT has the strongest bass score in these rows. BS-RoFormer is ahead in vocals, drums and other, but its four-stem average exceeds FT by only about 0.05 dB. SCNet improves vocals and drums over FT while giving up bass and other. These aggregates do not predict every electronic track or reveal audible artifacts.

SCNet-Large also appears as a different MSST-trained v1.0.8 checkpoint (9.19 Multisong average). Do not substitute that checkpoint while quoting the author's v1.0.9 scores. Likewise, do not compare the 9.70/9.65 MUSDB figures for SCNet/BS-RoFormer against FT's Multisong result.

## The other two families

- [BandIt/BandIt Plus](https://github.com/kwatcharasupat/bandit) and [Bandit v2](https://github.com/kwatcharasupat/bandit-v2) primarily publish cinematic dialogue/music/effects checkpoints. MSST's BandIt Plus 11.50 SDR is a DnR three-class result. It cannot be ranked as a four-stem music result. A specific music-trained checkpoint would need separate assessment; [Banquet](https://github.com/kwatcharasupat/query-bandit) is the related query-conditioned music model, with a different interface.
- [Kimberley Jensen's Mel-Band RoFormer](https://huggingface.co/KimberleyJSN/melbandroformer) scores 10.98 vocal SDR on the MSST Multisong evaluation, but this checkpoint outputs vocals/accompaniment. It can be evaluated as a vocal specialist alongside another four-stem model, rather than replacing a four-stem separator. The architecture supports multiple stems; MSST's [experimental multi-stem checkpoints](https://github.com/ZFTurbo/Music-Source-Separation-Training/blob/main/docs/mel_roformer_experiments.md) are a separate set of weights and results.

## Native integration and licenses

| Candidate | Current native status | Practical boundary |
| --- | --- | --- |
| HTDemucs FT | Historical baseline, superseded by BS-RoFormer | Four FP16 weight files total 662,450,544 bytes, excluding Basic Pitch; about four times the model work of standard HTDemucs |
| SCNet-Large | Not integrated in mixless; an [ONNX export](https://huggingface.co/audiojs/scnet-large) exists | Needs matching Rust STFT/iSTFT, normalization, overlap and provider parity; 41.2M parameters, published FP32 export 169.2 MB / FP16 weights 86.5 MB |
| BS-RoFormer 4stem | Implemented in Rust/ORT with MLX → CPU fallback | Single 269,457,840-byte FP16 graph; native STFT/iSTFT; fixed 8 s chunks; two real-valued spectral inputs and four joint outputs |
| Mel-Band RoFormer | Not integrated in mixless; ONNX tooling exists | A vocal specialist adds another inference pass; no local end-to-end timing or memory evidence yet |
| BandIt | Not integrated | The commonly distributed cinematic checkpoint does not meet the four-stem output requirement |

SCNet's author [explicitly permits redistribution of the pretrained weights and ONNX conversions under MIT](https://github.com/starrytong/SCNet/issues/35). BandIt implementations are Apache-2.0. BS-/Mel-RoFormer implementation code is MIT; review the chosen weight artifact separately. Kimberley's Hugging Face checkpoint is labeled MIT. An exporter's license does not by itself grant rights to another author's weights.

SCNet's 45.1 MB int8 ONNX artifact is a storage figure: its model card says weights are expanded to float32 at load. It does not prove a 45 MB resident session. No cross-model inference-speed or peak-memory ranking is claimed here.

## Current decision and measured boundary

The user selected BS-RoFormer 4stem to obtain all four stems from one jointly trained model, replacing the FT specialist bag. The four-stem MSST ep17 checkpoint is used through Silverdaw’s pinned host-STFT ONNX export. Native playback continues to use the original mixture at unity and includes reconstruction residual in Other when changing gains; a joint model does not mathematically guarantee mixture consistency. SCNet remains an efficiency candidate and Mel-Band a vocal-specialist experiment.

Historical FT smoke on Apple Silicon / MLX+CPU: 15 seconds of real stereo audio, model/session load 29.33 s, separation 13.65 s, note inference 1.66 s, residual RMS 0.01973. This establishes native execution and finite, aligned output, not subjective superiority. Results remain under ignored `target/verification/ft-native`.

Before switching again, use the same source files, decoder, overlap, normalization and output alignment for all checkpoints. Measure all four stems, remix/mute artifacts, cold/warm time and peak memory on the same Mac. Preserve dense electronic and sparse/vocal material, assess repeated recordings together, and keep published aggregate scores separate from local measurements and listening.


## BS-RoFormer integration evidence

The same 15-second source interval ran in native MLX+CPU: session load 9.89 s, separation 6.25 s, notes 0.14 s, joint-stem residual RMS 0.009989. These are local diagnostics for this interval and actual chunk policies, not a general quality ranking. CPU/MLX spectral max absolute difference was 4.96e-5. Native Rust STFT matched torch 2.12.1 within 3.06e-5; synthesizing the torch spectrum reconstructed the source within 2.99e-7. The production offline engine rendered unity bit-identically to the dry path; four individual mutes and a half-vocal gain were finite, and all-muted audio reached zero after the existing gain ramp.

The selected export declares MIT, while the original ep17 checkpoint's explicit redistribution grant remains unconfirmed in [MSST issue 249](https://github.com/ZFTurbo/Music-Source-Separation-Training/issues/249). Local download/execution and this development change are not a claim that public weight redistribution has been cleared. No release or mirror upload was performed.
