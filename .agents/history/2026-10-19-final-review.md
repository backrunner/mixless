# Metadata, stem and AutoMix review (2026-10-19)

Reviewed all pending tracked and untracked changes before batching them for
`main`. The scope includes filename preferences/downloading, recording metadata
recovery and staged tag writeback, source/audio hashes and cache consumers,
schema migration, package transfer, pruning, native BS-RoFormer transforms and
provider recovery, model notices, AutoMix descriptions/status, and bilingual
storage documentation. No blocking functional finding remains in this scope.

## Corrections

- Remove an unannounced FFmpeg dependency from the ordinary downloader test.
  Generate a minimal MP4 tag container in Rust instead; the regression still
  exercises format fallback, metadata publication and staging cleanup. This
  container test does not establish audio decodability.
- Address the pinned BS model in the damaged-weight regression, rather than
  leaving a damaged file under the retired HTDemucs name.
- Correct the provider-recovery test comment and format all changed Rust files
  with their package editions. Mark the earlier FT record as superseded.

## Local validation

- Workspace excluding desktop: 584 passed, 28 ignored.
- Desktop: 191 passed, 7 ignored.
- Controller tools: 89 passed, 4 ignored. Independent C/Wasm SDK replay: 8 frames,
  8 assertions. NI inspector: 9 tests; AutoMix evaluation observer: 24 tests.
- Strict translated site content: 24 pages, 134 search records, no errors/warnings.
- All 51 changed Rust files pass edition-aware rustfmt checks; staged and full
  diffs pass whitespace checks. `./dev.sh --build` passes.
- Final development bundle executable SHA-256:
  `8f8aef979c59f3656cb9b3fa97f2f29e652eb34634eef3e52898d21dd916456c`.
- Re-inspected the small Preferences capture and a fresh production-toolbar
  native preview on macOS 27.0.1. At 1180 logical pixels, all five lifecycle states
  retain the complete master controls. The capture is 2360 × 984 pixels in
  ignored `target/verification/final-review/automix-1180.png`; its executable
  identity and isolated offline data location are recorded alongside it. Both
  preview processes were stopped. No physical audio device was used.
- Earlier measured torch/native/provider parity, two full-track caches and
  production offline playback evidence remain in the
  [BS-RoFormer record](2026-10-19-bs-roformer.md). Numerical/engine checks do not
  establish subjective listening quality. A physical Traktor reimport was not
  performed.

Four Conventional Commits separate downloads, library/cache behavior, the BS
model and AutoMix UI/descriptions. This record is a fifth documentation commit.
The local GPG signing attempt was cancelled; the user requested skipping it.
These commits disable signing per invocation and preserve the local Git config.
Source audio, personal libraries, databases, build artifacts and model weights
are excluded. No release, model mirror or installed-app replacement is included;
the checkpoint redistribution boundary remains documented in the model record.
