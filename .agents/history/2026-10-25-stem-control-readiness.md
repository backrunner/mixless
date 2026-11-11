# Stem control readiness

Four stem controls remain visible in their existing 2×2 order. Missing or
unattached stem audio disables and dims the knobs and mute labels. A legacy
cache without independent bass enables the other three controls but keeps
BASS disabled. Availability follows the engine's attached playback audio, not
the presence of analysis metadata on disk.

`DeckSnapshot::stem_available` is shared by the deck and controller paths.
Disabled knobs have no mouse-down handler; state actions also reject drag and
reset input. Manual stem dispatch rechecks the fresh engine snapshot and track
identity, so an old rendered control cannot modify a replacement track. Polling
ends an existing stem drag when readiness is lost or the track changes.

Unavailable controller events are consumed without being queued for later UI
replay. Stem feedback is zero while unavailable. Target IDs, count, order and
mapping format are unchanged. The target catalogue and MIDI wireframe explain
the readiness requirement in hover help; learning stays available without
loaded audio. English/Chinese deck documentation describes the behavior.

## Validation

- Added four regressions: MIDI availability/feedback across both decks and all
  readiness combinations; consumed unavailable controller events; rendered
  control visibility and disabled/ready mouse interactions; stale input and
  an existing drag across track replacement.
- GPUI tests use synthetic PCM, including legacy and four-lane attached stem
  buffers. No separator inference, physical MIDI or listening result is claimed.
- Desktop suite: 219 passed, 7 ignored.
- Workspace excluding desktop: 643 passed, 28 ignored across 29 suites.
- `./dev.sh --build`, changed-file rustfmt checks and `git diff --check` passed.
- Logs: ignored `target/stem-controls-*.log`.
- Initial native inspection returned `cgWindowNotFound`. After the user
  confirmed the session was unlocked, the complete current build opened in an
  isolated offline workspace on macOS 27.0.1 (26A434). Both empty decks showed
  all four controls dimmed; mouse clicks/drags did not enable the unavailable
  stem controls. `assets/screenshots/stem-controls/unavailable-native.jpg` is
  an original 2880×1800 capture, with build details in `capture.json`.
- Legacy/ready stem lanes, stale gestures and MIDI availability are covered by
  synthetic-buffer GPUI/core tests, rather than claimed as native inference or
  hardware results. No audio device or listening performance was evaluated.
- Every owned test instance was stopped. The running user playback instance
  was not restarted and live library/audio files were not modified for testing.
