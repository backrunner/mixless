# Settings UI review

The [2026-10-11 follow-up review](../../../../crates/mixless-controllers/COMMUNITY_VERIFICATION.md#follow-up-review-and-fixes--2026-10-11)
records controller fixes, 124 passing desktop tests and native
minimum-window checks of import cancellation and preset menus.

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
