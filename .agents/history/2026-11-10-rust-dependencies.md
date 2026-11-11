# Rust dependency refresh (2026-11-10)

Baseline is the task-entry uncommitted workspace. Existing work is preserved.
`cargo upgrade --incompatible` updated direct requirements, followed by
`cargo update` for compatible transitive releases. Cargo.lock records the selected versions for reproducible builds.

## Principal resolved upgrades

| Dependency | Before | After |
| --- | --- | --- |
| CPAL | 0.15.3 | 0.17.1 |
| Symphonia | 0.5.5 | 0.6.1 |
| rusqlite | 0.32.1 | 0.40.2 |
| rfd | 0.15.4 | 0.17.2 |
| dirs (direct) | 6.0.0 | 7.0.0 |
| rtrb | 0.3.4 | 0.4.1 |
| lofty | 0.22.4 | 0.24.0 |
| ureq (direct) | 2.12.1 | 3.5.1 |
| reqwest | 0.12.28 | 0.13.5 |
| sha2 (direct) | 0.10.9 | 0.11.1 |
| zstd | 0.13.3 | 0.14.3 |
| zip | 2.4.2 | 9.5.0 |
| mp3lame-encoder | 0.2.5 | 0.3.0 |
| ebur128 | 0.1.10 | 0.1.11 |
| which (direct) | 7.0.3 | 8.0.6 |
| libloading (direct) | 0.8.9 | 0.9.0 |
| base64 (direct) | 0.22.1 | 0.23.1 |

The complete before/after package version inventory is in ignored
`target/planner-perf-20261110/dependency-versions.json`. Older transitive
major versions remain where upstream consumers require them. Rayon 1.12.0
was already locked transitively and is now a direct planner dependency.

## Compatibility constraints

- GPUI 0.2.2 pins core-foundation exactly 0.10.0; midir 0.11's CoreMIDI
  dependency requires 0.10.1. Keep midir 0.10.4.
- CPAL 0.17.2/0.17.3 and 0.18 require alsa-sys 0.4 through ALSA,
  conflicting with midir 0.10's alsa-sys 0.3 native `links` value. Cargo
  resolves this graph across target-specific dependencies even on macOS.
  Precision upgrade attempts were rejected; 0.17.2 is also yanked.
- protobuf stays exactly 3.4.0 for onnx-protobuf's generated bindings.
  Existing FFmpeg and ONNX Runtime pins and local source patches are retained.
- Minimum declared Rust is now 1.89, required by ogg_pager 0.7.2 through
  Lofty. Metadata reports no dependency requiring a newer compiler. Actual
  checks use Rust 1.99.1; Rust 1.89 itself was not installed/tested.

## Source adaptations

- CPAL's sample-rate and device-description APIs retain existing saved device
  names, channel routing and buffer policy. Symphonia's decoder/probe/packet
  API is migrated explicitly: source decoding keeps gapless trimming disabled,
  while stem decoding keeps it enabled and verifies exact frame counts,
  channel alignment, finite samples and lossless checksums.
- Audio identity v2 hashes binary codec metadata instead of upstream Debug
  formatting. Existing database identity mappings continue to address existing
  stem caches. Tag writeback validates the staged audio using the current
  algorithm while preserving an existing v1 cache identity; regression coverage
  exercises both old and new mappings.
- ureq 3 keeps status responses available for retry handling. A local HTTP
  regression verifies a 429 preserves Retry-After and its body. System-proxy
  routing, explicit bypass, timeout and artwork redirect policies are retained.
- Lofty picture/tag APIs and rusqlite backup identifiers are migrated. SHA-256
  text encoding remains lowercase hexadecimal with the same bytes.

Validation results are recorded in the companion planner-performance report.
No live library migration, playback-device acceptance, external authentication
check or remote CI run is claimed by these local checks.
