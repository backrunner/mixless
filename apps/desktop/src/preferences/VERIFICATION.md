# Settings UI review

## Concise stem storage copy — 2026-10-15

The selected format now shows `32-bit float WAV`, `24-bit FLAC. Slight precision
loss.`, or `320 kbps MP3`, followed by `New stems only.`. The detailed float-to-PCM
conversion explanation remains in the storage documentation.

Both General preference tests passed, including all choices, persistence and
940 × 820 / 720 × 540 bounds. The required workspace check passed 476 tests
(26 ignored), and the development bundle built without warnings.

On macOS 27.0.1 (26A434), native mouse input switched through all three formats
at 940 × 820. The minimum 720 × 540 window reopened the saved FLAC choice and
scrolled to its complete controls and short notes. Original native captures:
[standard window](../../../../assets/screenshots/stem-storage-concise.jpg) and
[minimum window](../../../../assets/screenshots/stem-storage-concise-small.jpg).
Build identity, binary hash and dimensions are recorded in capture.json.
The isolated offline preview processes were stopped after verifying PID, UID
and executable paths. This copy-only review did not involve audio hardware.

## Stem storage choices — 2026-10-15

General → Library adds No compression (default WAV), Lossless (24-bit FLAC at
level 8), and Lossy (320 kbps MP3). The FLAC selection explicitly explains
32-bit float to 24-bit PCM rounding. New analysis jobs use the saved choice;
existing caches are kept. Reset defaults restores the live processor to WAV,
and failed settings writes retain the preceding format.

The complete workspace check passed 475 tests (26 ignored); desktop passed
150 (7 ignored). An additional 19 x86_64 codec/package tests passed under
Rosetta (1 ignored). The development bundle built without warnings. Bilingual
content checks, site type checks and the static site build passed. See the
[codec and real-audio evidence](../../../../crates/mixless-stems/VERIFICATION.md)
for exact-length decoding, DSP, cache/package compatibility and measured sizes.

On macOS 27.0.1 (26A434), native mouse selected and saved FLAC and MP3 in the
940 × 820 production Preferences window. Reopening at 720 × 540 preserved MP3;
native scrolling reached the complete setting and mouse selection switched to
FLAC, with the precision note fully visible and the saved value checked.
The isolated preview processes were stopped after checking PID, UID and exact
executable paths. These checks used temporary offline data, not the user library.

Original 2× JPEGs: [WAV](../../../../assets/screenshots/stem-storage-wav.jpg),
[FLAC](../../../../assets/screenshots/stem-storage-flac.jpg),
[MP3](../../../../assets/screenshots/stem-storage-mp3.jpg), and
[minimum window](../../../../assets/screenshots/stem-storage-small.jpg).
Build identity, dimensions and binary SHA-256 are in capture.json.

## AutoMix playlist selection — 2026-10-15

General → AutoMix now offers On playlist change: Follow selected (the existing
default) or Keep current. The setting persists across launches and applies to
subsequent playlist selections. The running source is independent of browsing,
including All Tracks. Background refreshes still update that source; unrelated
reorders and removals leave it running. Retries use the source queue even when
the visible playlist is empty. Removing the source stops AutoMix, and toggling
AUTO off and on starts from the visible playlist.

Desktop tests: 149 passed, 7 ignored. Workspace excluding desktop: 465 passed,
24 ignored. Regressions cover both modes, stale asynchronous reads, an empty
source, All Tracks, reordering another playlist, source edits/deletion, actual
offline retry loading, defaults and saved settings. GPUI exercises both choices
and persisted values at 940 × 820 and 720 × 540. The development bundle built
with zero warnings. Bilingual content, type checks and static site build passed.

On macOS 27.0.1 (26A434), native mouse input selected Keep current in the
940 × 820 production Preferences view; the saved value and selected state after
reopening were verified. See [default choice](../../../../assets/screenshots/automix-playlist-follow.jpg)
and [Keep current](../../../../assets/screenshots/automix-playlist-keep.jpg).
Both are original 1880 × 1640 JPEG captures, with build/hash records in
capture.json. The isolated 720 × 540 window launched, but native wheel input
did not scroll to AutoMix; small-window interaction evidence is from GPUI.
Testing used temporary offline data and synthesized audio in the offline engine;
no physical audio device, listening or frame-time measurements are claimed.
The isolated preview processes were inspected by PID, UID and executable path
before cleanup. User library data and running playback were not modified.

The [2026-10-11 follow-up review](../../../../crates/mixless-controllers/COMMUNITY_VERIFICATION.md#follow-up-review-and-fixes--2026-10-11)
records controller fixes, 124 passing desktop tests and native
minimum-window checks of import cancellation and preset menus.

## Structured HID import preview — 2026-10-12

Import review now has a compact document header and preset name, separate
author/version/license columns with a source button, and short type/control/
feedback fields. Device requirements use aligned port/protocol/value rows,
including optional HID interface and Usage constraints. Output and feature
permissions use a Port / Access / Report ID / Bytes table. No inline dot
separators or decorative nested cards were added.

The detail body scrolls independently. Cancel, the filled Import preset button,
the copy/apply explanation and bounded save errors remain in a pinned footer.
Long names and metadata stay on one line with their full value in a tooltip.
Import still saves a separate preset and does not apply it to any device.
The import renderer is separated from edit dialogs and persistence actions.

Checks on macOS **27.0.1** passed: **128 desktop tests (7 ignored)**,
**442 workspace tests (24 ignored, excluding desktop)**, the development build,
workspace formatting and `git diff --check`. The existing import regression
now covers 128 report permissions, long names/author fields, naming conflicts
and a simulated long save error at 940 × 820 and 720 × 540. Scrolling reaches the last
permission without moving confirmation controls. Existing cancellation,
explicit import, read-only adapter mapping and import error/retry checks pass.

Native visual/mouse verification remains pending. The desktop was locked;
the window tool returned `cgWindowNotFound` for the distinct isolated preview
bundle, both by path and bundle ID. No new screenshot was captured or added to
the capture registry. GPUI render-tree/layout checks establish automated
geometry and interaction coverage, not native pixel appearance or OS mouse
delivery. The isolated offline preview was stopped after checking its identity;
the user's applications and data were not changed. Earlier captures remain
historical evidence of the prior import layout.

## Responsive controller details — 2026-10-11

Device/preset selection occupies the left column; expanded preset type, mapping
and feedback counts, validation and related actions occupy the right column.
Below an 848 px window width, details stack beneath the choices. The choices
keep their width and button alignment when Details toggles. Long detail values
use one line with the full value available in a tooltip. Controller value menus
follow the trigger width rather than retaining the old 280/320 px minimum.

The development build, **124 desktop tests (6 ignored)**, **432 workspace tests
(24 ignored, excluding desktop)**, desktop formatting and `git diff --check` passed.
The existing mapping regression checks both sides of the responsive breakpoint,
column bounds, usable preset field width, stable Details/Manage alignment and
the anchored preset menu. During implementation, it caught an intrinsic-width
collapse at the minimum window size; explicit panel/choice widths resolved it.
Workspace-wide rustfmt still reports an existing module-order difference in
`mixless-protocol/src/lib.rs`; that unrelated file was not changed in this UI revision.
The subsequent pre-commit review corrected that ordering; workspace formatting now passes.

Native mouse checks at 940 × 820 verified Details expansion/collapse without
moving the choices and repeated-click closure of the aligned preset menu.
At 720 × 540, details stacked beneath the choices and collapsed correctly; the
permanent layout remained below the header and Done remained visible.
Both isolated offline preview processes exited successfully on Escape.

Original 2× native JPEGs:
[default](../../../../assets/screenshots/controller-columns/default.jpg),
[two columns](../../../../assets/screenshots/controller-columns/details.jpg),
[preset menu](../../../../assets/screenshots/controller-columns/preset-options.jpg)
and [minimum window](../../../../assets/screenshots/controller-columns/small.jpg).
Source build/hash, dates and dimensions are in
[capture.json](../../../../assets/screenshots/capture.json). The distinct test
bundle was ad hoc signed and used temporary data, preserving the user's running
application and settings. These are UI checks, with no controller hardware attached.

## Details/select indicator alignment — 2026-10-11

Details now shares Manage's 88 px width and the select's 8 px right inset.
Disclosure buttons anchor their indicator at the right edge rather than
centering the label/indicator together. The existing 12 × 16 px vector boxes
and 1.25 px strokes remain consistent. Closed and expanded arrows stay in the
same column as the selects above.

The development build, **124 desktop tests (6 ignored)**, **432 workspace tests
(24 ignored, excluding desktop)**, formatting and `git diff --check` passed.
The existing interaction test checks that Details and Manage share both edges
and that expansion preserves the Details button's size. Native prepared-state
captures confirm the alignment when
[closed](../../../../assets/screenshots/controller-icons/default.jpg) and
[expanded](../../../../assets/screenshots/controller-icons/expanded.jpg).
Coordinates clicked through the automation did not toggle this isolated preview
window during this pass; these images establish rendering, while toggle behavior
was verified by GPUI tests. Both preview processes exited successfully on Escape.
Build/hash, dates and dimensions are in
[capture.json](../../../../assets/screenshots/capture.json).

## Recognizable controller buttons — 2026-10-11

Details, MIDI inputs, Preset notes and Input monitor share a bordered, filled
32 px disclosure button. Labels stay fixed while a vector chevron and stronger
border show the expanded state. Hover uses the shared button surface. Removed
the overrides that made controller actions look like plain text; recommended
presets now use a compact, left-aligned button.

Verification passed: **124 desktop tests, 6 ignored**; **432 workspace tests,
24 ignored** (excluding desktop); the development build, formatting and
`git diff --check`. The existing mapping interaction test now checks stable
Details/MIDI inputs button dimensions through expansion and collapse, and
confirms that opening MIDI inputs preserves the permanent diagram below it.
Existing device tests cover notes/monitor disclosure and assignment isolation.

Native mouse checks at 940 × 820 confirmed Details, Preset notes and MIDI inputs
expand and collapse, keep their labels/positions and display the selected
border and chevron. At 720 × 540 the buttons fit horizontally and MIDI inputs
opens/closes correctly. The inactive test window first required keyboard focus;
mouse checks were performed after activation. Done closed both preview processes.
Checks used a distinct ad hoc signed test bundle with isolated offline data and
did not change the user's running application or settings. No controller hardware
was attached; these are UI checks.

Original 2× native JPEGs:
[default](../../../../assets/screenshots/controller-buttons/default.jpg),
[Details expanded](../../../../assets/screenshots/controller-buttons/details.jpg),
[MIDI inputs expanded](../../../../assets/screenshots/controller-buttons/inputs.jpg)
and [minimum window](../../../../assets/screenshots/controller-buttons/small.jpg).
Capture dates, dimensions and source build/hash are recorded in
[capture.json](../../../../assets/screenshots/capture.json).

## Always-visible controller layout — 2026-10-11

Controllers now always renders the clickable layout above the mapping list.
The Show/Hide layout button and its state have been removed, including the
preview harness override. MIDI inputs and device details remain disclosures.
Small windows scroll the layout and mapping workspace together while the tabs
and Done footer remain fixed. Stem labels have more space at the minimum width.
The English and Chinese mapping guides describe the permanent layout.

Verification passed: **124 desktop tests, 6 ignored**; **432 workspace tests,
24 ignored** (excluding desktop); the development build, formatting,
`git diff --check` and strict bilingual content checks. Existing dependency
future-compatibility and engine deprecation warnings remain.

The GPUI tests now exercise the layout without opening a disclosure at
720 × 540, 940 × 820 and 1200 × 900. Mapping group and Learn/editor tests scroll
to their controls before clicking, and the all-tab test verifies that the final
mapping controls remain reachable with the footer inside the window.

Native checks on an unlocked desktop used an isolated, offline test bundle with
a distinct identifier, preserving the user's running application and settings.
At 940 × 820, clicking Deck B Play selected its mapping and input without arming
offline learning. At 720 × 540, the permanent layout and separate Stem labels
rendered correctly; scrolling reached the entire mapping list and inspector.
Done stayed visible and closed both preview processes successfully.

Four original 2× JPEG captures record the final build:
[default layout](../../../../assets/screenshots/controller-layout/default.jpg),
[selected mapping](../../../../assets/screenshots/controller-layout/mappings.jpg),
[minimum window](../../../../assets/screenshots/controller-layout/small.jpg) and
[minimum mapping workspace](../../../../assets/screenshots/controller-layout/small-mappings.jpg).
Dates, dimensions, source build identity and binary SHA-256 are in
[capture.json](../../../../assets/screenshots/capture.json). The test clone is
ad hoc signed, so its signed binary hash differs from the recorded source hash.
These checks establish UI rendering and interaction, not hardware compatibility.

## Previous revision: selects, switches and concise device details — 2026-10-10

- General, Audio I/O, Controllers and Storage use quiet dividers, compact rows,
  smaller button/menu radii and neutral selected states. Removed rounded group
  cards and shortened explanatory copy without removing destructive-action or
  audio-interruption details.
- Controllers opens directly to the mapping list and inspector. The controller
  diagram and MIDI input configuration are optional disclosures. Device/preset
  fields have a bounded width; one Manage menu holds new, import, duplicate,
  edit, export, rename and delete. Built-ins omit rename/delete, and editing
  continues to save a user copy.
- Preset options show names rather than repeated built-in/verification text.
  Duplicate names retain a distinguishing type; tooltips and Details retain
  verification details. Identically named devices retain an instance
  identifier when needed. The interface never upgrades protocol/simulation
  evidence to hardware verification.
- Selects and buttons share a 32 px height, aligned vector chevrons and clear
  borders. Value selectors and action menus have distinct surfaces; primary
  actions use the same filled style. Menu rows are 30 px. Their menus stay
  attached to the trigger, flip above when necessary, constrain to the window,
  and scroll the current selection into view. Repeated-click, outside-click,
  keyboard, focus/resize dismissal and scroll isolation are preserved.
- Mapping groups use the shared segmented control. Switching groups resets the
  list position and selects an input in the new group, keeping the inspector in sync.
- Selecting a device shows its connection status and active preset. Details uses
  short label/value rows; protocol notes and the raw input monitor require another
  explicit disclosure. Changing the device or preset closes these disclosures.
  Generated settings labels no longer use middle-dot separators.
- Offline and complex/layered mapping inspection shows actual sources rather than
  an editable dummy MIDI address. Clicking the diagram offline only selects a
  target. HID devices use the MIDI/HID preset editor. Connected MIDI devices retain
  Learn; editing an inspected mapping opens the assigned preset even when another
  preset has been selected but not applied.
- Audio keeps routing and format controls visible, with diagnostics under Device
  details. Storage descriptions no longer expose database implementation details.

## Automated evidence

- `cargo test --locked -p mixless-desktop`: **123 passed, 6 ignored**.
- Previous controller-module run: **27 passed, 2 ignored**. That crate was not
  changed in this UI revision.
- Development build, formatting and `git diff --check` pass. Existing engine
  deprecation and dependency future-compatibility warnings remain.

Twelve GPUI tests exercise the actual Preferences render tree:

1. Controller diagram geometry at 720 × 540, 940 × 820 and 1200 × 900.
2. Offline preset selection; Manage actions; no implicit device assignment;
   modal background isolation and Unicode preset names.
3. Import error/retry/cancel/accept; invalid edit preserves the previous preset.
4. MIDI-first device menu order; scrolling a long menu keeps the page stationary.
5. Trigger anchoring, pointer departure, toggling/switching, outside-click
   isolation, Escape/resize dismissal and disabled headphone channel selection.
6. Selected-item visibility in a 128-item list, upward placement, keyboard
   selection, outside-scroll isolation and clipped-trigger dismissal.
7. Mouse caret/drag selection for preset names; reset confirmation/cancellation.
8. All four pages at three sizes, long footer errors, scrolling to the final
   controls and cache-clear cancellation.
9. Default mapping workspace visibility, concise options, builtin action limits,
   optional diagram/details, offline diagram clicks and keyboard action menus.

10. Device selection keeps notes/packet monitor closed, limits notes height,
    clears disclosures on preset/device changes and preserves other assignments.
11. Mapping segments stay within the viewport at three sizes; inspector selection
    follows the active group and the primary controls keep their common height.
12. A complex assigned mapping never exposes a dummy Note 0; failed learning leaves
    the assignment intact, and its editor opens the assigned preset.

These are synthetic interaction tests, not unlocked-desktop mouse evidence.

## Native rendering evidence

The debug-only `--settings-preview SCENE [WIDTH HEIGHT]` harness opens the same
production Preferences view. It requires an isolated directory under the system
temporary directory and both offline flags. It initializes a completed device/storage scan,
selects an offline DDJ-400 preset where appropriate, and measures the production
controls before opening a menu. Connected scenes use a CoreMIDI virtual source
and destination named `mixless UI Preview`, selected through the explicit
`MIXLESS_PREVIEW_MIDI_INPUT` environment variable. They use real transport discovery
and an assignment saved only in the isolated configuration; no physical controller
is represented as tested. The harness is excluded from release builds.

Sixteen unmodified 2× macOS window captures were inspected:

| Page/state | Native PNG |
| --- | --- |
| General | [Default](../../../../assets/screenshots/settings-controls/general.png), [720 × 540](../../../../assets/screenshots/settings-controls/general-small.png) |
| Audio I/O | [Default](../../../../assets/screenshots/settings-controls/audio.png), [720 × 540](../../../../assets/screenshots/settings-controls/audio-small.png) |
| Controllers | [Default](../../../../assets/screenshots/settings-controls/controllers.png), [720 × 540](../../../../assets/screenshots/settings-controls/controllers-small.png) |
| Storage | [Default](../../../../assets/screenshots/settings-controls/storage.png), [720 × 540](../../../../assets/screenshots/settings-controls/storage-small.png) |
| Preset choices | [Menu](../../../../assets/screenshots/settings-controls/preset-options.png) |
| Preset actions | [Menu](../../../../assets/screenshots/settings-controls/preset-actions.png) |
| Audio buffer | [Menu](../../../../assets/screenshots/settings-controls/audio-options.png) |
| Controller details | [Expanded](../../../../assets/screenshots/settings-controls/controller-details.png) |
| Controller layout | [Expanded](../../../../assets/screenshots/settings-controls/controller-layout.png) |
| Connected virtual MIDI | [Default](../../../../assets/screenshots/settings-controls/controller-connected.png), [720 × 540](../../../../assets/screenshots/settings-controls/controller-connected-small.png) |
| Connected device details | [Expanded](../../../../assets/screenshots/settings-controls/controller-connected-details.png) |

Default captures are 1880 × 1640; minimum-size captures are 1440 × 1080. Exact
build identity, binary SHA-256 and capture times are recorded per image in
[capture.json](../../../../assets/screenshots/capture.json). Temporary paths and
empty caches belong to the isolated test configuration. These are review images,
not marketing captures. Historical screenshots and their metadata remain intact.

The Mac remained locked (`CGSSessionScreenIsLocked = true`). Native captures
prove rendering of these prepared states; they do not prove OS mouse delivery or
physical-controller compatibility. The user explicitly deferred unlocking.
An unlocked desktop pass for pointer movement, trackpad input and focus changes
remains a separate verification step.

## Earlier evidence

The 2026-10-11 overall review additionally checked all four settings tabs and
community import/mapping at default and minimum sizes, then exercised native
menus, keyboard selection, mapping groups and scrolling on an unlocked desktop.
See [the current review record](../../../../crates/mixless-controllers/COMMUNITY_VERIFICATION.md#overall-review-and-fixes--2026-10-11)
for the twelve captures, exact tested interactions and their limits.

Community adapter import, preset details and read-only mapping rendering were
checked subsequently at both window sizes. The six native captures, build
identity, GPUI test results and locked-desktop limits are recorded in
[Community adapter verification](../../../../crates/mixless-controllers/COMMUNITY_VERIFICATION.md).

`settings-general.png`, the original controller captures and `settings-review/`
belong to preceding UI revisions. Their timestamps and build records were not
relabeled as evidence for this revision.

## NI mapping and headphone mix — 2026-10-12

The editor accepts Pitch Bend and explicit 14-bit CC pairs, with relevant
calibration fields behind Calibration. Relative sensitivity and jog resolution
are only shown for controls that use them; absolute controls expose pickup.
Audio I/O includes Headphone mix, and the diagram/catalogue share CUE MIX.

The 125 desktop tests passed after the change. The new regression exercises
message/channel/MSB/LSB selection, invalid pairing errors, editor bounds at
940 × 820 and 720 × 540, and applying a stored zero mix over a MIDI-updated
engine value. NI tests use synthetic messages and an offline UI identity.

Native screenshot review is **not completed**: the host was locked and the
window service returned `cgWindowNotFound`. No screenshot was substituted with
a mockup. The isolated preview was stopped. See the
[controller validation record](../../../../crates/mixless-controllers/VERIFICATION.md#ni-offline-development--2026-10-12)
for the build and other checks.

## NI audio source expansion — 2026-10-12

The 13 exact NI controller names with per-device output pairs confirmed in the
Traktor source audit now receive an Audio routing proposal. The existing control
opens an Audio draft for review, including without a control preset; it does not
apply settings or assign a controller automatically.

Desktop tests: **127 passed, 6 ignored**, included in the 567-test workspace
result. New render-tree tests use fictional controller identities and a disabled
hub to check the routing control at 940 × 820 and 720 × 540, click-through to
Audio settings, preservation of stored output settings, and absence for an
unknown NI generation. GPUI 0.2.2 retains old debug bounds across frames, so the
absence assertion uses a fresh window. Native pixel review remains incomplete
while the host is locked; no physical audio output was tested.

## Learn lifecycle review — 2026-10-12

Manual message/channel/CC-half changes now disarm pending Learn, and shortcut
Learn uses the same explicit-pair arming path as the editor. Workspace checks
include **128 desktop tests passed, 7 ignored**. The new ignored GPUI/CoreMIDI
test was additionally run explicitly and passed using a single selected virtual
source: MSB alone does not complete Learn, LSB completes the full address, and
the saved calibration remains intact. No real controller was opened. The host
remains locked, so these checks do not add native pixel evidence.

## Mapping scroll isolation and build warnings — 2026-10-13

The mapping list consumes wheel input before stopping propagation to the
enclosing settings page, including at both list edges. The GPUI regression
reproduced the original outer-page movement before the fix, then passed at
940 × 820 and 720 × 540. It also checks that scrolling outside the list still
moves the page.

An unlocked native macOS pass exercised middle scrolling, overscroll at both
edges and the outer page margin. The unchanged list frame can be compared in
the [first rows](../../../../assets/screenshots/mapping-scroll-before.jpg),
[middle rows](../../../../assets/screenshots/mapping-scroll-middle.jpg) and
[last rows](../../../../assets/screenshots/mapping-scroll-end.jpg) captures.
Build identity and binary hash are recorded in
[capture.json](../../../../assets/screenshots/capture.json).

The locked workspace suite passed 452 tests (24 ignored); the desktop suite
passed 131 tests (7 ignored). The native CoreMIDI virtual hotplug, feedback and
headless dispatch test was run separately and passed. These results include
the local dependency compatibility patches documented in
[third-party/rust](../../../../third-party/rust/README.md). No physical controller
was attached. The complete `./dev.sh --build` succeeded with zero warning lines
in `target/verification/mapping-scroll/build.log`.

## None preset and concise names — 2026-10-13

None is the first preset option and the empty selection label. It clears an
offline preview immediately; with an assigned device, Apply persists removal
of that device's assignment. Other assignments remain intact. The GPUI test
checks mouse and keyboard selection, both window sizes and store reload.
Template suffixes are omitted from primary preset labels, recommendations and
the active-preset summary. IDs and stored names remain unchanged; requirements
stay in Details.

The unlocked native pass selected Traktor Kontrol F1 by mouse, confirmed the
short name, then used Home and Return to return to None. See the
[None menu](../../../../assets/screenshots/preset-none-menu.jpg) and
[device name](../../../../assets/screenshots/preset-device-name.jpg) captures.
Workspace tests: 452 passed, 24 ignored. Desktop tests: 132 passed, 7 ignored.
The complete development build passed with zero warnings, recorded in
`target/verification/preset-none/build.log`. These UI checks used no assigned
physical controller and add no audio measurement evidence.

## Initial storage usage loading — 2026-10-13

The first Storage scan now shows an amber spinner and concise calculating
message before rendering sizes and cache actions. A request made while the
initial device scan is running stays queued and starts when that job finishes.
Repeated tab selection does not restart the scan. Scan errors and disconnected
workers end loading and leave a retry action. Existing usage stays visible
during later refreshes.

The new GPUI regressions cover both window sizes, queued device discovery,
actual asynchronous storage report completion, failure/retry and worker exit.
Workspace tests: 452 passed, 24 ignored. Desktop tests: 134 passed, 7 ignored.
The complete development build passed with zero warnings; see
`target/verification/storage-loading/build.log`.

Native macOS pixel review used the production loading renderer in an isolated,
offline debug preview with the pending state intentionally held. Successive
frames showed spinner rotation. See the
[default view](../../../../assets/screenshots/storage-loading.jpg) and
[720 × 540 view](../../../../assets/screenshots/storage-loading-small.jpg).
Original capture dimensions, build identity and binary hashes are recorded in
[capture.json](../../../../assets/screenshots/capture.json). These prepared
states do not establish first-open mouse delivery. The normal development
launch stalled in the self-install location write check before creating a
window, and the preview mouse clicks did not produce a verified tab change.
The stopped verification processes were inspected by PID and command before
cleanup. No cache data was cleared and no audio measurements were taken.

## DJ controller catalogue and natural names (2026-10-13)

The controller library adds 24 independently authored DDJ profiles, for 39 total
(34 Pioneer DJ / AlphaTheta). Standalone mixers and decks are excluded. Each new
model has an official PDF fingerprint, per-model omissions and a pinned synthetic
replay. The 24 replays assert all 2,420 frames; they are not hardware captures.
See [source and coverage records](../../../../crates/mixless-controllers/examples/alphatheta-ddj/README.md).

Device/preset choices sort model numbers naturally, preserving exact model
generations and assignment IDs. Recognized MIDI names get canonical display names;
unknown names retain their identity. Duplicate devices retain instance labels.
None stays first, and NI template requirements remain in the original metadata.
GPUI checks cover natural order, normalized duplicates, stable IDs, offline
selection, None, menu scrolling and small-window layouts. Workspace tests: 460
passed, 24 ignored. Desktop tests: 136 passed, 7 ignored. Developer tool test: 1
passed. The full development build and bilingual site content/type/build checks
passed without warnings.

Native macOS review used the fresh production Preferences renderer in isolated,
offline preview bundles. Mouse and wheel opened/scrolled the default menu and
mouse selection switched to DDJ-FLX6, updating the diagram. The small-window menu
was visually inspected, but subsequent native input delivery was intermittent;
keyboard behavior is covered by GPUI, not claimed from native key delivery.
See [natural order](../../../../assets/screenshots/preset-natural-order.jpg),
[small closed view](../../../../assets/screenshots/controller-presets-small.jpg)
and their build/hash records in capture.json. These images precede the final SX3
disconnect-packet correction, which does not change the displayed menu. No device
firmware, calibrated jog, physical LED or audio routing validation is claimed.
The isolated preview processes were checked by PID, UID, start time and executable
path before stopping. User assignments and installed-app data were not changed.

## Batch preset import (2026-10-13)

Manage → Import uses the native multi-file JSON picker. File reading and adapter
validation run on the background executor. Single-file review is retained; batch
review lists valid files, failure reasons and naming conflicts, with per-preset
Details for community metadata and report permissions. Back returns to the batch;
Cancel/Escape leaves the library unchanged. While saving, controls are disabled.
Valid presets are revalidated and committed in one transaction with fresh IDs;
failed validation or persistence leaves the entire batch unsaved. Current device
assignments and active bindings are preserved. Invalid source files are skipped
only after their results are shown for review.

Five controller regressions cover mixed files, duplicate IDs/names, changed
validation/adapter failures, storage rollback/retry, persistence and limits. Three
GPUI regressions exercise actual modal rendering and actions at 940 × 820 and
720 × 540: Details/Back/Escape, modal background blocking, real asynchronous
write failure/retry, retained assignments, a 40-file scrolling list with pinned
footer, and an all-invalid batch. GPUI retains removed debug selector bounds,
so absence is checked through review state and library behavior rather than a
stale bound. Half-pixel layout rounding is allowed within one logical pixel.

Workspace: 465 passed, 24 ignored. Desktop: 139 passed, 7 ignored. Development
build: zero warnings. Bilingual content, type and static build checks passed.
The installed rfd 0.15.4 macOS implementation explicitly enables multiple-file
selection; the production call compiles. Native picker interaction and screenshots
are not established: CUA reported a locked Mac and failed automatic unlock.
An isolated offline debug batch-preview scene is ready for later native review.
See target/verification/preset-batch-import/verification.json for build/hash data.
No physical controllers or user preset files were imported during testing.


### Controller routing restoration (2026-10-13)

- Reproduced the reported flow in the actual GPUI Preferences render tree:
  select DDJ-FLX4, click Audio routing, return to Controllers, select None.
  Before the fix, the draft still referenced DDJ-FLX4 and its output channels.
- Audio recommendations now retain the preceding draft routes. Selecting None,
  another preset or another controller restores the unchanged suggested routes,
  including stable UIDs and channel pairs. Manual output edits and audio format
  edits remain intact. Repeated recommendations preserve the original baseline.
- A successful Audio I/O Apply saves the original output routes alongside the
  suggested route. None automatically restores saved outputs and persists them;
  a fresh Preferences view reconstructs the backup after reopening. Output
  failure retains the old selection and backup for retry. A settings write
  failure rolls the device back before reporting the error. Unapplied format
  changes remain drafts. Old settings without the backup field keep loading.
  Save/restore transactions are tested with a simulated device and real settings
  persistence; worker completion messages are injected without opening hardware.
- Six regression tests cover the rendered click/keyboard flow at 940 x 820 and
  720 x 540, repeated recommendations, preset/device changes, NI audio proposals
  without a control preset, manual output/format changes and save results.
- Local validation: 465 workspace tests passed (24 ignored), 145 desktop tests
  passed (7 ignored), and dev.sh --build completed with zero warnings.
- Native interaction review was attempted using the fresh build and isolated
  temporary data. CUA reported the Mac locked and automatic unlock unavailable;
  no new native screenshot or physical audio-routing evidence is claimed.
  Build metadata and reproduction evidence are recorded under ignored
  target/verification/controller-routing/verification.json.
