# Settings UI review — 2026-10-10

## Current revision: selects, switches and concise device details

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

`settings-general.png`, the original controller captures and `settings-review/`
belong to preceding UI revisions. Their timestamps and build records were not
relabeled as evidence for this revision.
