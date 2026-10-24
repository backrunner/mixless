# Rust compatibility patches

These crates are pinned copies of the published registry packages and are
selected with `[patch.crates-io]`. `UPSTREAM.json` retains each original package
checksum. Original authorship, license metadata and upstream source are retained.
The first two patches address Rust 1.99 future-incompatibility diagnostics.
The FFmpeg patch fixes the source revision and the audio build configuration.

- [`block` 0.1.6](https://github.com/SSheldon/rust-block), MIT: declare the foreign
  `_NSConcreteStackBlock` symbol with an inhabited byte type and obtain its
  address using `addr_of!`. The block's `isa` field still holds the same symbol
  address as an opaque class pointer. No byte is read and no reference to an
  uninhabited value is constructed. Foreign declarations and callback types
  explicitly name the C ABI, preserving the previous implicit ABI.
- [`proc-macro-error2` 2.0.1](https://github.com/GnomedDev/proc-macro-error-2),
  MIT selected from MIT/Apache-2.0: make `extern crate proc_macro` public, matching
  its existing public re-export.

Published development-only test harness dependencies are omitted from these
copies; they are not workspace members. The application build and native
runtime checks exercise the patches. The standalone MIT notices are also copied
into the app bundle by mixless-tools.

Remove these overrides when compatible upstream releases become available.
Do not edit Cargo's shared registry cache to apply these changes.

- [`ffmpeg-sys-next` 9.0.1](https://github.com/zmwangx/rust-ffmpeg-sys),
  WTFPL as declared upstream: verify FFmpeg 9.0.2 against a fixed Git commit,
  compile only local audio processing with portable CPU settings, avoid legacy
  Apple video framework links, select the SDK for macOS cross compilation, and
  export corresponding sources for bundling. The build recipe invalidates its
  native cache when changed. `MIXLESS_FFMPEG_SOURCE_DIR` supports modified-source
  rebuilds without network access. See [FFmpeg-BUILD.md](../FFmpeg-BUILD.md).
