# Built-in audio processing

mixless links FFmpeg 9.0.2 into its Rust acquisition module. It does not launch
ffmpeg or ffprobe, search Homebrew for those tools, or load a system FFmpeg
library. yt-dlp still handles service metadata and downloads; Deno remains an
optional JavaScript runtime for yt-dlp.

## Source and license

- Upstream: https://github.com/FFmpeg/FFmpeg
- Tag: `n9.0.2`
- Commit: `946fcce07b6dcd0331c8cc609192aeff5e1924f8`
- Library license for this configuration: LGPL-2.1-or-later.
- Rust wrappers: ffmpeg-next 9.0.0 and ffmpeg-sys-next 9.0.1, WTFPL as declared
  by their packages. Cargo.lock pins the registry wrapper; the local sys crate's
  UPSTREAM.json records the original registry package checksum.

FFmpeg sources are unmodified. The patched sys crate verifies the tag's commit,
builds static libraries and exports a clean source archive before compilation.
The packaging tool copies that archive to
`Mixless.app/Contents/Resources/Sources/ffmpeg-9.0.2-source.tar.gz` and preserves
the license, component notices and this document in `Resources/Licenses/`.
For a custom-source build, that archive contains the supplied source tree.
Distribute these materials with the corresponding application source and build
instructions so the application can be rebuilt and relinked with changed code.

## Configuration

The complete executable build recipe is in
`third-party/rust/ffmpeg-sys-next-9.0.1/build.rs`. It enables avcodec, avformat,
avutil, avfilter and swresample, with baseline CPU settings for the Cargo target.
It disables autodetection, programs, documentation, network access, GPL/nonfree
components and optional external codecs. No x264, x265 or system codec packages
are linked. No video decoder or encoder is enabled.

Enabled components:

| Kind | Components |
| --- | --- |
| Decoders | AAC, AAC fixed-point/LATM, Opus, Vorbis, MP3, FLAC, signed 16-bit and float 32-bit PCM |
| Encoder | AAC |
| Demuxers | MOV/MP4/M4A, Matroska/WebM, MPEG-TS, ADTS AAC, Ogg, WAV, MP3, FLAC |
| Muxer | iPod/M4A |
| Parsers | AAC, AAC LATM, Opus, Vorbis, MPEG audio, FLAC |
| Bitstream filter | AAC ADTS to AudioSpecificConfig |
| Protocol | Local files only |
| Audio filters | Buffer source/sink, format conversion, resampling, pass-through |

AAC is remuxed without re-encoding. Alternate audio is converted to AAC at
256 kbit/s for stereo or 128 kbit/s for mono, preserving the source sample rate.
All processing runs on import workers, outside the realtime audio callback.
Temporary outputs are removed on failure; recording metadata is written before
the completed M4A file is published.

## Build and relink

Normal builds need Git, Make and Clang (provided by full Xcode on macOS), plus
the existing Rust/CMake project prerequisites. The first build downloads the
pinned source; later builds reuse the native Cargo output. ffmpeg/ffprobe
executables are not build prerequisites. Intel macOS cross builds use the Xcode
macOS SDK and do not need NASM.

To rebuild offline or with modified FFmpeg sources, extract the bundled source
archive into a clean source directory, make the desired source changes, and set
`MIXLESS_FFMPEG_SOURCE_DIR` to its absolute path:

```sh
MIXLESS_FFMPEG_SOURCE_DIR=/absolute/path/ffmpeg-9.0.2 ./dev.sh --build
```

Cargo copies that tree into its own build directory before compiling; it does
not compile in or delete your source directory. Keep generated objects and other
unrelated files out of the supplied tree. Changing the source tree or build
recipe rebuilds the library. The resulting app is relinked with those libraries
and its source archive reflects that supplied tree. Remove the variable to
return to the verified upstream build.

Run `cargo test --locked -p mixless-acquire-yt` for native conversion, packet
preservation, signal/channel, metadata, error and cleanup checks. Live download
tests are explicitly ignored by default and require yt-dlp and service access.

Before distribution, inspect each executable architecture with `otool -L` and
verify that no Homebrew or external libavcodec/libavformat/libavutil/libswresample
libraries are referenced. Keep local compilation, cross compilation, live
service tests and physical Intel execution evidence distinct.
