# Third-party notices

mixless source is licensed under MPL-2.0; see LICENSE.

The vendored Signalsmith DSP and stretch sources in
`crates/mixless-engine/native/vendor/` retain their own MIT licenses and copyright
notices. Consult the LICENSE files alongside those sources when redistributing.

Rust dependencies are pinned in Cargo.lock and retain their respective licenses.
Local compatibility patches for `block` 0.1.6 (Steven Sheldon, MIT) and
`proc-macro-error2` 2.0.1 (CreepySkeleton and GnomedDev, MIT selected from
MIT/Apache-2.0) are retained under `third-party/rust/`. Their standalone MIT
notices are included in `third-party/` and copied into the app bundle.
This document is not a complete transitive dependency license audit. Review
licenses and required notices for the actual binaries before publishing releases.

Optional acquisition and conversion use separately installed yt-dlp
(Unlicense) and FFmpeg (LGPL or GPL depending on the build configuration).
They are not bundled by this repository, and their use is governed by the
terms of the services they access. Imported audio remains subject to its own
rights.

Stem cache encoding uses [libflac-sys](https://github.com/mgeier/libflac-sys)
0.3.4 with its bundled libFLAC 1.5.0 (BSD-3-Clause) and [mp3lame-encoder](https://github.com/DoumanAsh/mp3lame-encoder)
0.2.5 with [mp3lame-sys](https://github.com/DoumanAsh/mp3lame-sys) 0.1.11
(LGPL-3.0), which statically links its unmodified LAME 3.100 sources
(LGPL-2.0-or-later). Decoding uses Symphonia (MPL-2.0). Codec license texts
are preserved in `third-party/` and copied into the app bundle. The exact
Rust packages and bundled LAME sources are available through `cargo vendor
--locked`; build instructions are in CONTRIBUTING.md. To rebuild with modified
codec sources, use Cargo's `[patch.crates-io]` path overrides and rebuild the
application. Preserve these notices and the corresponding sources/rebuild
materials when distributing a binary containing these libraries.

Native model inference uses [ort](https://github.com/pykeio/ort) (MIT/Apache-2.0)
and [ONNX Runtime](https://github.com/microsoft/onnxruntime) (MIT), linked into
the application. Apple Silicon builds also bundle ONNX Runtime 1.29 and
[onnxruntime-ep-mlx](https://github.com/onnxruntime/onnxruntime-ep-mlx),
[MLX](https://github.com/ml-explore/mlx), and
[mlx-c](https://github.com/ml-explore/mlx-c) (all MIT) as optional native
libraries and Metal kernels. Their licenses and the 1.29 third-party notices
are included in `third-party/`. These components do not require Python.
Model artifacts are included in the installer with models, or
downloaded on first use by the standard installer: the single four-stem BS-RoFormer ONNX graph exported
by [Silverdaw](https://huggingface.co/silverdaw/bs-roformer-rhythm-onnx), based on
[MSST](https://github.com/ZFTurbo/Music-Source-Separation-Training) and
[lucidrains BS-RoFormer](https://github.com/lucidrains/BS-RoFormer) (MIT code), and
[Spotify Basic Pitch](https://github.com/spotify/basic-pitch) (Apache-2.0).
The model I/O and windowing implementation consults these upstream projects;
note event decoding is implemented in Rust. MSST and BS-RoFormer code licenses
are retained as `MSST-LICENSE.txt` and `BS-RoFormer-LICENSE.txt`. The chosen
export labels the weights MIT; an explicit upstream grant for redistributing
the original checkpoint is still pending in MSST issue 249. No public model
release is established by this development change. The licenses and ONNX Runtime
third-party notices are retained in `third-party/` and copied into the local app
bundle by `mixless-tools`. Pinned artifacts and inference boundaries are recorded
in `.agents/reference/native-inference.md`. The old Python evaluation environment
is not a runtime or build dependency.

Controller profiles in `crates/mixless-controllers` are independently authored
from manufacturer protocol specifications. Source URLs, document versions,
sections, and verification limits are recorded in that crate's `SOURCES.md`.
Manufacturer manuals are referenced, not redistributed. No Mixxx mapping,
script, importer, or derived preset is shipped. This provenance statement does
not replace a license review of a release's complete dependency graph.

MIDI transport uses [midir](https://github.com/Boddlnagg/midir) (MIT).
HID transport uses [hidapi-rs](https://github.com/ruabmbua/hidapi-rs)
(MIT) and its bundled [HIDAPI](https://github.com/libusb/hidapi) native backend
(BSD-3-Clause selected from its licensing alternatives). Preserve their notices
when distributing binaries.

Community HID adapters run in [Wasmi](https://github.com/wasmi-labs/wasmi)
2.0.0 and its companion crates (MIT selected from MIT/Apache-2.0).
The Wasmi MIT notice is preserved in `third-party/wasmi-LICENSE-MIT.txt`.
Validation uses wasmparser from [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
(MIT selected from its licensing alternatives); its notice is preserved in
`third-party/wasm-tools-LICENSE-MIT.txt`.
The desktop interpreter does not enable WASI or JIT compilation. Imported
community presets retain their own author, source and license information;
an import is not an endorsement or a hardware certification.

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
