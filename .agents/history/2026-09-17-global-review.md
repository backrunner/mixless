# Global review — 2026-09-17

Review baseline: `c72c9f0a967806f409d5d6486d872da3acb4c663`. The review covered
deck/master gain and limiters, AutoMix planning/publication, playback/loading,
recording, library persistence, acquisition, and the updater/release path.
It does not establish that the application has no remaining bugs.

## Corrected findings

- **AutoMix publication deadline (P1):** preparation could succeed while the
  outgoing deck advanced past the plan's launch time. The subsequent commit
  failed outside the retry loop and stopped AUTO. Planning, preparation and
  guarded publication now share a bounded three-attempt retry; each attempt
  reads the current playhead and stem readiness without consuming another
  playlist entry. A deterministic regression delays the first publication,
  then verifies that the same pair replans and finishes.
- **Acquisition directory deletion (P1):** starting a download recursively
  removed every `.mixless-*` directory in its destination, including another
  active download or a user-created directory. Each download now relies on
  its own `TempDir` cleanup. A cached-fetch regression verifies that both
  categories of neighboring directories survive.
- **Incompatible-library backup (P1):** second-resolution backup names could
  collide, and ignored WAL/SHM rename failures could separate journals from
  their database before a fresh library opened. Backups now use unique sibling
  directories, check every move and roll back on failure. A failed rollback
  preserves the remaining backup files; startup does not open a fresh database
  after a failed backup. Tests cover collisions, failed moves and failed rollback.
- **Installer lock compatibility (P2):** `File::try_lock` requires Rust 1.89,
  newer than the workspace's stated Rust 1.88 minimum. The installer now uses
  the existing `fs2` dependency's exclusive lock, retaining serialization
  through activation. This removes that API mismatch; a full Rust 1.88 build
  was not performed.
- **Recording tail loss (P2):** the writer could observe an empty queue, then
  stop after the final callback had published more frames. It now observes
  the producer's terminal state before draining and finalizing the WAV. A
  deterministic test publishes the last stereo frame at the stop observation
  and verifies the sealed WAV contains it. The audio callback is unchanged.

## Validation

Machine: macOS 27.0 (26A428), Rust 1.98.1, Xcode.app developer directory.

The updater baseline's GitHub Actions run
[35204752514](https://github.com/backrunner/mixless/actions/runs/35204752514)
completed successfully, including workspace and desktop tests. The signed
package checks and their limits remain documented in the
[updater audit](2026-09-17-updater.md).

| Local check | Result |
| --- | --- |
| Full workspace excluding desktop (`--locked --no-fail-fast`) | 281 passed, 0 failed, 11 ignored |
| Desktop, including AutoMix retry, library recovery and installer locking | 72 passed, 0 failed, 2 ignored |
| Recording regressions, explicitly run after the writer correction | 5 passed, 0 failed, 1 ignored |
| `./dev.sh --build` | Passed |
| Changed Rust formatting and diff whitespace | Passed |

The initial workspace run stopped at the downloader's two-second wall-clock
assertion during severe machine load. The complete rerun above passed without
changing the timeout code or its assertion. The desktop suite ran before the
recording-writer-only correction; that correction is covered by the focused
recording run and the subsequent complete workspace run.

Source fixes are committed as `2b96f3f7d9ceadb4e68da78ec733aa3374686af4`, initially
pushed on `fix/global-review-20260917`. Independent macOS CI is tracked in
[35210809812](https://github.com/backrunner/mixless/actions/runs/35210809812).
Logs are local artifacts under `target/global-review-*.log`.

## Unresolved real-time timing results

The serial release callback suite ran before the recording-writer correction:
**1 passed, 4 failed**. None of this review's fixes change DSP or audio-callback
processing. The timing assertions and thresholds were not weakened.

| Workload | Observed p99 | Required bound | Result |
| --- | --- | --- | --- |
| FX catalogue, Echo, 256 frames | 0.1725 ms | < 0.12 ms | Failed |
| Dual deck, 256 frames | 46.379 ms | < 2.667 ms | Failed |
| Retriggered cues, 128 frames | 251.303 ms | < 1.333 ms | Failed |
| Recording, 128 / 512 frames | 0.506 / 1.489 ms | < 1.333 / 5.333 ms | Passed |
| Stems, pitch, FX, recording and cues, 128 frames | 2.184 ms | < 1.3335 ms | Failed |

Other projects' compiler/audio jobs, simulator services and disk-image work
were active. A later machine snapshot showed load averages above 400. These
conditions violate the idle-machine prerequisite in CONTRIBUTING.md, but do
not prove all failures are environmental. Real-time performance remains
unverified until the unchanged budgets pass on a quiet machine; successful
functional tests do not resolve these failures. No hardware playback,
subjective listening or native UI interaction was validated by this review.
