# Library metadata, source identity and FT update (2026-10-19)

This records the intermediate FT implementation and its validation. The later
[BS-RoFormer update](2026-10-19-bs-roformer.md) supersedes the model selection,
analysis version and final desktop-test result below.

## Scope and behavior

- Reproduced 17 affected rows in the 29-track MUG playlist: the embedded title was a remote ID and the artist was missing, while linked import evidence retained the correct recording identity. Protect metadata on reimport and repair affected rows on library open.
- New downloads choose Artist - Title, Title, or Recording ID; readable names retain a recording ID suffix for collision safety. Persist selected naming in Preferences. New audio contains the recording title/artist/ISRC before publication.
- Complete import batches, local imports, album backfills and finished desktop analysis write known title/artist/album/ISRC/BPM/key tags. Write to a staged source copy, preserve other tags, verify audio identity, atomically replace, and migrate source/analysis/waveform hashes together. Read-only or unsupported sources retain metadata in the library. Import-batch writeback runs after parallel analysis jobs join.
- Full-file BLAKE3 remains source/package integrity. Stems use decoder configuration and every encoded audio packet, excluding container tags; identities are memoized by verified file hash in additive schema v4. Audio changes invalidate stems; names, paths and tag-only edits do not. Package transfer and scoped storage accounting/cleanup use the same audio identities.
- Default separation is the checksum-pinned four-member HT-Demucs FT bag, with each member contributing only its specialist row. Include every member in model cache identity and give CoreML graphs distinct identities. Basic Pitch remains v0.4.0. Analysis v22 rebuilds old stem-adjusted features before applying new evidence.

## Local evidence

- macOS 27.0.1, Apple Silicon, MLX+CPU; no physical listening/audio-device claim.
- Backed up the database and 77 source files before maintenance. Wrote known tags to all 77 sources with zero failures. A separate FFmpeg encoded-audio hash comparison against every backup found zero mismatches. A final maintenance pass wrote zero files.
- Removed 37,895,218,342 bytes of retired stems and 165,612,636 bytes of retired standard-model weights. Referenced current FT caches remain. Removed the disposable isolated library/source test copies after verification; original recovery backups remain under ignored `target/verification/library-maintenance-20261019`.
- FT native 15-second stereo smoke: load 29.33 s, separation 13.65 s, notes 1.66 s, residual RMS 0.01973. This is execution evidence, not a local model-quality ranking or listening test.
- Native Preferences inspected at 940 x 820 and 720 x 540; naming controls fit and scroll. Actual PNGs and capture/build metadata: ignored `target/verification/download-preview/`.
- Workspace excluding desktop: 584 passed, 28 ignored. Additional final library/stems regressions: 69 passed, 6 ignored. Desktop: 190 passed, 7 ignored when the current workspace's unrelated AutoMix toolbar-width test is excluded; that full-suite test failed at width 1180, with the master control extending beyond the toolbar. This is not an all-desktop-tests-pass claim.
- `dev.sh --build` completed. Bilingual content validation: 24 pages, 134 search records, zero errors/warnings. `git diff --check` passed.

No app release, remote CI result, physical Traktor reimport or five-model same-machine comparison is claimed. The researched checkpoint comparison and its source links are in [stem-model-comparison.md](../reference/stem-model-comparison.md).
