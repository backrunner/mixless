# Third-party notices

Mixless source is licensed under MPL-2.0; see LICENSE.

The vendored Signalsmith DSP and stretch sources in
`crates/mixless-engine/native/vendor/` retain their own MIT licenses and copyright
notices. Consult the LICENSE files alongside those sources when redistributing.

Rust dependencies are pinned in Cargo.lock and retain their respective licenses.
This document is not a complete transitive dependency license audit. Review
licenses and required notices for the actual binaries before publishing releases.

Optional acquisition and conversion use separately installed yt-dlp and FFmpeg.
Their licenses depend on their versions/build configurations; they are not
bundled by this repository. Imported audio remains subject to its own rights.

Native model inference uses [ort](https://github.com/pykeio/ort) (MIT/Apache-2.0)
and [ONNX Runtime](https://github.com/microsoft/onnxruntime) (MIT), linked into
the application. Model artifacts are downloaded separately: HTDemucs exported
by [StemSplit](https://github.com/StemSplit/demucs-onnx) (MIT), based on
[Meta Demucs](https://github.com/facebookresearch/demucs) (MIT), and
[Spotify Basic Pitch](https://github.com/spotify/basic-pitch) (Apache-2.0).
The model I/O and windowing implementation consults these upstream projects;
note event decoding is implemented in Rust. Their licenses and the ONNX Runtime
third-party notices are retained in `third-party/` and copied into the local app
bundle by `mixless-tools`. Pinned artifacts and inference boundaries are recorded
in `.agents/reference/native-inference.md`. The old Python evaluation environment
is not a runtime or build dependency.
