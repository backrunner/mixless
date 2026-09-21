# Third-party notices

mixless source is licensed under MPL-2.0; see LICENSE.

The vendored Signalsmith DSP and stretch sources in
`crates/mixless-engine/native/vendor/` retain their own MIT licenses and copyright
notices. Consult the LICENSE files alongside those sources when redistributing.

Rust dependencies are pinned in Cargo.lock and retain their respective licenses.
This document is not a complete transitive dependency license audit. Review
licenses and required notices for the actual binaries before publishing releases.

Optional acquisition and conversion use separately installed yt-dlp
(Unlicense) and FFmpeg (LGPL or GPL depending on the build configuration).
They are not bundled by this repository, and their use is governed by the
terms of the services they access. Imported audio remains subject to its own
rights.

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

## Trademarks

"Spotify" is a trademark of Spotify AB. "YouTube" and "YouTube Music" are
trademarks of Google LLC. "SoundCloud", "Bandcamp", "rekordbox" and "djay" are
trademarks of their respective owners. References in this repository are for
interoperability and comparison only; mixless is not affiliated with, endorsed
by, or sponsored by any of these parties.

## Brand assets

The mixless name and the artwork under `assets/branding/` (including the app
icon, which was produced with AI-assisted generation; provenance is documented
in `assets/branding/README.md`) identify the project. The MPL-2.0 license of
the source code does not grant rights to use the mixless name or logo to imply
endorsement or to distribute confusingly similar builds.
