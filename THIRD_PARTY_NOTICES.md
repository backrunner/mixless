# Third-party notices

Mixless source is licensed under Apache-2.0; see LICENSE.

The vendored Signalsmith DSP and stretch sources in
`crates/mixless-engine/native/vendor/` retain their own MIT licenses and copyright
notices. Consult the LICENSE files alongside those sources when redistributing.

Rust dependencies are pinned in Cargo.lock and retain their respective licenses.
This document is not a complete transitive dependency license audit. Review
licenses and required notices for the actual binaries before publishing releases.

Optional acquisition and conversion use separately installed yt-dlp and FFmpeg.
Their licenses depend on their versions/build configurations; they are not
bundled by this repository. Imported audio remains subject to its own rights.
