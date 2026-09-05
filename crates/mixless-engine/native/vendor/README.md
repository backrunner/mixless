# Vendored Signalsmith DSP

Unmodified algorithm headers from the published `signalsmith-stretch` **0.1.3**
crate: Signalsmith Stretch **1.3.1** and its bundled Signalsmith Linear headers.
Only the required headers, macOS FFT backend, and both MIT licenses are retained.
The Rust wrapper from that crate is not included.

Upstream repositories:

```text
https://github.com/Signalsmith-Audio/signalsmith-stretch
https://github.com/Signalsmith-Audio/linear
https://github.com/colinmarc/signalsmith-stretch-rs
```

The local bridge enables custom-window `splitComputation`, uses interleaved
stereo and deterministic initialization, and preallocates before stream startup.
No dependencies are fetched by the build script. Keep upstream license files
with these headers when distributing source or binaries.
