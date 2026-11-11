# Downgrade safety

## Behavior

Startup checks the last authorized app/build, schema, analysis, automatic cue,
waveform, Best order, stem evidence/storage and inference runtime generations.
It also inspects actual database generations, stem manifests and downloaded
model identities when no version ledger exists. Unchanged model/cache files
reuse inspected metadata instead of being hashed or decoded on every launch.

Newer or incompatible data requires an explicit warning recommending the latest
mixless. The intended native default is Quit; Return and Escape keep the newer
data. Only Continue and replace data authorizes conversion. The native keyboard
and screenshot checks remain unverified; see the validation boundary below.

No engine, MIDI hub, normal Library migration or model worker starts before the
gate completes. Diagnostics still return before the gate. Install-only checks
do not inspect or migrate library data. Replacing a newer installed app from a
disk image has a separate explicit warning before the bundle is copied.

## Conversion and recovery

- Backups are retained under the data folder's `downgrade-backups`. SQLite's
  backup API includes committed WAL pages. Preferences, MIDI configuration, the
  original version ledger and affected models/stem caches are preserved there.
- Database conversion happens in a temporary supported-schema library, with
  atomic SQLite activation. Newer schemas are rebuilt without unknown triggers
  or foreign keys; compatible derived rows can be retained. Tracks, source
  paths, playlist membership/order, manual cues, library identity and retired
  autoincrement IDs are retained. Unmappable known user fields stop conversion
  rather than opening an empty library.
- A live source connection checks `data_version` and the source file identity.
  The final revision check runs after SQLite acquires the destination write
  lock, preventing lost edits from legacy apps that do not honor the new
  process lease. Busy writers fail promptly. Failed conversion rolls back
  reversible model/cache/preference moves.
- Session, model and analysis locks prevent cooperating instances or inference
  jobs from racing conversion. Downloaded model names from foreign ledgers
  cannot escape the model directory or name lock files. Bundled models are
  separate from the writable downloaded-model directory.
- Incompatible preferences retain independently compatible fields. Generated
  caches are replaced by this version's results; manual cues remain user data.
  The successful version stamp records the accepted writer, avoiding repeated
  downgrade warnings on subsequent launches.

This protection applies to executables containing the gate. It cannot add a
warning to previously published older executables or installers.

## Validation

All data tests use synthetic temporary libraries and model/cache fixtures.
No live library or source audio was edited, and no running playback instance
was restarted. No listening or playback performance result is claimed here.

Final local checks:

- `cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=1`:
  642 passed, 28 ignored across 29 suites.
- `cargo test --locked -p mixless-desktop -- --test-threads=1`:
  211 passed, 7 ignored.
- Targeted library downgrade regressions: 10 passed (included above).
- `./dev.sh --build`: passed. Refreshed `target/app/Mixless.app`, version
  `0.1.0-beta.5`, dev channel, build time `2026-10-24T16:23:17Z`.
- Changed-file rustfmt checks and `git diff --check`: passed.
- `--build-info` returned noninteractively with the protected native-warning
  fixture's hashes unchanged. Final binary identity and SHA-256 are recorded
  in `target/downgrade-audit/manifest.json`.

The library regressions cover read-only inspection, cancellation boundaries,
WAL-inclusive backups, additive/nonadditive schema conversion, retained manual
cues and retired IDs, unsupported user fields, independent cue/model versions,
busy writers, commits after backup, the activation write lock, and unknown
future triggers that would otherwise delete user records.

Desktop regressions cover acceptance/refusal, normal upgrades and first launch,
absence of a ledger, unknown stem layouts/codecs, preferences projection,
failed-conversion rollback, inference/process contention and foreign model
paths. Raw logs and the isolated native-warning fixture are under the ignored
`target/downgrade-audit` directory.

Initial native attempts returned `cgWindowNotFound` and LaunchServices `-10810`.
After the user confirmed the session was unlocked, native verification passed
on macOS 27.0.1 (26A434), using a complete, independently identified review
bundle. A test-only launcher sets explicit isolated data/model paths and offline
flags before starting the current executable. The earlier failure's exact cause
was not established; it is not attributed to missing libraries or the product.

- The real NSAlert rendered with Quit as the default action. Return exited;
  asking CUA for that exited app's AX state automatically relaunched it, as
  confirmed by the second startup log. Escape and clicking Quit also exited.
- Protected fixture hashes were unchanged after cancellation, with no backup.
- Clicking Continue and replace data opened the native workspace, created
  exactly one backup whose protected files matched the original hashes, and
  stamped the current app/build/analysis versions. Relaunch opened the workspace
  without another warning or backup.
- Native captures are in `assets/screenshots/downgrade-safety/`; dimensions, build
  identity and capture context are in `assets/screenshots/capture.json`.
- The later combined desktop/workspace suites passed 219/643 tests, with 7/28
  ignored. Build time: `2026-10-24T17:19:34Z`. Raw native logs and hash assertions
  are in ignored `target/native-review-20261025/`.

Every owned test process was stopped. The original live playback PID remained
running throughout. No audio device or listening performance was evaluated.
