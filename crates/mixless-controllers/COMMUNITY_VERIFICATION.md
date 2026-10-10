# Community adapter verification — 2026-10-10

Latest follow-up results are recorded at the end of this document.

Local checks on macOS 27.0.1 (26A434), using Rust 1.99.0. No physical DJ
controller was available. These results establish implementation, simulation and
native rendering coverage, not hardware certification.

## Automated checks

| Check | Result |
| --- | --- |
| `cargo test --locked --workspace --exclude mixless-desktop --quiet` | 419 passed, 24 ignored |
| `cargo test --locked -p mixless-desktop --quiet` | 124 passed, 6 ignored |
| `cargo test --locked -p mixless-controllers --features tools --quiet` | 41 library tests and 1 tool/schema test passed, 4 ignored |
| `python3 crates/mixless-controllers/sdk/verify-example.py` | C compiled to Wasm, packed, validated and replayed; 8 frames, 8 expectations passed |
| `./dev.sh --build` | Development application bundle built successfully |
| `pnpm check:content` in `apps/site` | 24 pages, 124 search records; no errors or warnings |

The final controller and desktop runs include the bounded document reader,
metadata limits and selected-preset lookup optimization. The workspace run
preceded those final localized changes. The SDK check used Apple Clang and the
Wasm linker bundled with the Rust toolchain; it exercised the actual C header
and compiler output, not only a hand-written Wasm fixture.

Coverage includes adapter import cancellation and acceptance, explicit device
assignment, independent copies for rollback, restart and portable export,
invalid-module rejection, report permissions, fuel limits, held-control release,
worker overflow, late results after reassignment and fault isolation across
devices. Trace tests cover content fingerprints, strict expected effects,
timers, disconnect handling, truncation and removal of track/device metadata.
Both synthetic programmable and declarative HID examples use independently
invented protocols. Neither example describes a real supported device.

The existing engine deprecation and dependency future-compatibility warnings
remain. Development packaging is not installation, release signing or
notarization. The CI workflow now runs the tooling and C SDK checks; a remote CI
result is not claimed by this local record.

## Dispatch timing and virtual MIDI

Timing tests ran separately from builds and preview applications:

| Test | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Minimal adapter, receive through isolated worker to host action dispatch | 13 µs | 17 µs | 24 µs |
| CoreMIDI virtual input, callback to handler | 130 µs | 166 µs | 383 µs |

`isolated_adapter_dispatch_latency` passed for 500 events. The `virtual_` test
selection passed all three tests, including the opt-in recording round trip.
These figures exclude physical USB transport and audio buffering. The minimal
adapter measurement does not promise the same latency for every community
program. No audio callback or DSP implementation changed in this revision.

## Native UI rendering

The development binary built at `2026-10-10T13:47:46Z` was launched with an
isolated temporary configuration and offline settings. Its SHA-256 was
`55e4053fe12f1faf37a9c32ee8695b5009ff5fbe14e4edc5e5173632c93400af`.
Six actual macOS window captures were inspected:

| View | Default window | Minimum 720 × 540 window |
| --- | --- | --- |
| Import preview | [Image](../../assets/screenshots/community-controllers/adapter-import.png) | [Image](../../assets/screenshots/community-controllers/adapter-import-small.png) |
| Preset details | [Image](../../assets/screenshots/community-controllers/adapter-details.png) | [Image](../../assets/screenshots/community-controllers/adapter-details-small.png) |
| Adapter mapping | [Image](../../assets/screenshots/community-controllers/adapter-mapping.png) | [Image](../../assets/screenshots/community-controllers/adapter-mapping-small.png) |

Import actions remain visible at the minimum size. The mapping inspector shows
concise read-only adapter ownership and a source link. Details identify dynamic
feedback as adapter-controlled. The minimum-size details body scrolls while the
footer stays separate. Images are unmodified native 2× captures, with individual
dimensions and timestamps in [capture.json](../../assets/screenshots/capture.json).

The desktop was locked during the final captures. They verify rendering of
prepared states, not OS mouse delivery or hover/focus behavior. GPUI tests cover
the new import and mapping interactions. Physical HID input/output/feature
reports, firmware differences, sleep/wake and controller audio routing still
require real hardware; no compatibility status was promoted by these checks.

## Overall review and fixes — 2026-10-11

Reviewed import validation, sandbox capabilities and limits, state ownership,
device lifecycle, queued I/O, recording/replay, SDK execution and settings UI.
The review found and fixed these issues:

| Issue | Correction |
| --- | --- |
| A declarative mapping could release a target still held by a Wasm adapter, or vice versa | Check both sources of hold ownership before delivering release, including adapter faults |
| Queued declarative HID writes could survive reassignment; their late results could overwrite the replacement's feedback error | Cancel output sessions when retiring an assignment and ignore retired completion results; order best-effort cleanup before new initialization |
| HID enumeration, opening and report-descriptor reads could block the performance dispatcher | Poll asynchronous requests after initial inventory; keep blocking operations on the persistent discovery worker |
| Disabling controllers omitted a recorded disconnect; lost queue events could leave a trace marked complete | Record shutdown boundaries and mark captures incomplete when events are lost |
| A shared queue overflow released holds but could subsequently replay an older queued press | Discard packets received before recovery, while accepting subsequent input |
| The C SDK check could miss an installed Wasm linker when Homebrew Rust preceded rustup on PATH | Query rustup's selected compiler as a fallback without changing the user's toolchain |

Five new regression tests cover both orders of mixed-source release, normal
release and adapter faults, cancelled output and stale success/error replies,
recorded shutdown and queue loss, pending discovery with ongoing dispatch, and
overflow recovery followed by fresh input. The SDK check exercises the compiler
and linker fallback end to end.

Final local results on macOS 27.0.1 (26A434):

- Workspace excluding desktop: **424 passed, 24 ignored**.
- Desktop, including GPUI settings tests: **124 passed, 6 ignored**.
- Controllers with `tools`: **46 library tests and 1 schema/tool test passed,
  4 ignored**.
- C SDK example, without `CLANG` or `WASM_LD` overrides: **8 frames and
  8 expectations passed**.
- Clippy with `-D clippy::correctness`: passed. Existing style, deprecation and
  dependency future-compatibility warnings remain.
- Formatting, `git diff --check`, site content validation and `./dev.sh --build`:
  passed. Site content: **24 pages, 124 search records, no errors or warnings**.
- Isolated minimal adapter timing: **p50 9 µs, p95 23 µs, p99 52 µs**.
- All three virtual CoreMIDI tests: passed. Dispatch timing:
  **p50 62 µs, p95 106 µs, p99 168 µs**.

Timing runs were separate from builds and previews and exclude physical USB
transport and audio buffering. No audio callback or DSP code changed. Packaging
refreshed the development bundle; installation, release signing and remote CI
are not claimed.

### Current native rendering and interaction

Twelve unmodified native screenshots were captured and inspected from the
development build at `2026-10-10T16:53:24Z`, SHA-256
`f7ce46aadb680b9d526f49b1d84e0aaa1034c5a67745886e22fd2e192089c2ac`.
The desktop was unlocked. Each view used an isolated offline test configuration.

| View | 940 × 820 window | 720 × 540 window |
| --- | --- | --- |
| General | [Image](../../assets/screenshots/community-review/general.png) | [Image](../../assets/screenshots/community-review/general-small.png) |
| Audio I/O | [Image](../../assets/screenshots/community-review/audio.png) | [Image](../../assets/screenshots/community-review/audio-small.png) |
| Controllers | [Image](../../assets/screenshots/community-review/controllers.png) | [Image](../../assets/screenshots/community-review/controllers-small.png) |
| Storage | [Image](../../assets/screenshots/community-review/storage.png) | [Image](../../assets/screenshots/community-review/storage-small.png) |
| Community import | [Image](../../assets/screenshots/community-review/adapter-import.png) | [Image](../../assets/screenshots/community-review/adapter-import-small.png) |
| Adapter mapping | [Image](../../assets/screenshots/community-review/adapter-mapping.png) | [Image](../../assets/screenshots/community-review/adapter-mapping-small.png) |

Native pointer, wheel and keyboard actions in a separate isolated minimum-size
window also verified:

- The preset menu anchors to its trigger, scrolls to the last entry and does not
  scroll the underlying mapping page.
- Clicking outside dismisses the menu without activating the underlying layout
  button. Escape dismisses menus. Down followed by Enter selects DDJ-FLX4; Apply
  remains disabled with no device selected.
- Switching to Mixer updates the mapping table and selected control inspector.
  The Manage menu remains inside the viewport.
- After scrolling Audio I/O, the Buffer size menu opens above its trigger and
  remains inside the window.
- General and Storage scroll to their lower controls; the fixed footer remains
  usable. Done closes the test application successfully.

These checks found no new layout or tested interaction failures. The app exposes
only the window and window controls in its accessibility tree, so native content
checks used screenshots and coordinate input. This does not establish screen
reader support. Individual image dimensions, capture times and build identity
are retained in [capture.json](../../assets/screenshots/capture.json); earlier
screenshots remain historical evidence. No physical DJ controller was used.

## Follow-up review and fixes — 2026-10-11

Continued the review of session boundaries, mixed-source holds, timer ordering,
replay recovery, legacy migration and import validation. Eight further issues
were corrected:

| Issue | Correction |
| --- | --- |
| Input received before a preset switch or restart could be routed through its replacement | Track the input boundary for each affected port; leave unrelated controllers' queued input available |
| Reapplying a multi-interface declarative preset initialized only the primary interface; cancelled feedback on secondary interfaces remained cached | Initialize every assigned interface and invalidate feedback for all ports at the logical session boundary |
| Enqueueing a HID retry cleared its previous output failure before the device had accepted the report | Clear errors on successful I/O completion; best-effort cleanup cannot overwrite the current error |
| An expired timer could be queued while an earlier callback was replacing its deadline, producing a trace that could not replay | Wait for earlier callback results before dispatching the timer; unchanged deadlines still fire after the pending callback completes |
| Hot cues and temporary cues were treated as unrelated holds even though they end the same deck preview | Share preview hold ownership per deck across declarative mappings and adapters; retain independent deck ownership |
| A declarative replay error retained held controls and decoder state across reconnect | Release and clear router state on replay failure, matching adapter fault cleanup |
| Blank source entries bypassed the community provenance requirement | Require nonblank document URL, revision and section for every community protocol source |
| Editing an explicit legacy wildcard mapping left its previous control held and could reinterpret older queued input | Retire only wildcard binding state and its queued input, preserving device-specific binding holds |

Eight new regression tests cover these changes. Lifecycle and shared test setup
were separated from transport dispatch. Source fields establish required metadata;
they do not verify the publisher's claims or establish license clearance.

Final local checks on macOS 27.0.1 (26A434):

| Check | Result |
| --- | --- |
| Workspace excluding desktop | **432 passed, 24 ignored** |
| Desktop, including GPUI settings tests | **124 passed, 6 ignored** |
| Controllers with `tools` | **54 library tests and 1 schema/tool test passed, 4 ignored** |
| C SDK example, without compiler/linker overrides | **8 frames, 8 assertions passed** |
| Clippy with `-D clippy::correctness` | Passed; existing style/deprecation and dependency future-compatibility warnings remain |
| `./dev.sh --build` | Development bundle refreshed successfully |

The timing checks ran separately from builds and preview applications:

| Path | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Minimal Wasm adapter, receive to action dispatch (500 events) | 10 µs | 28 µs | 42 µs |
| CoreMIDI virtual input, callback to handler | 112 µs | 178 µs | 271 µs |

All three opt-in virtual MIDI checks passed, including hotplug/feedback and
recording round-trip coverage. These results exclude physical transport and
audio buffer latency. No audio callback or DSP implementation changed.

The final build (`2026-10-10T17:54:41Z`, dev `0.1.0-beta.4`, revision
`v0.1.0-beta.4-2-ga020455-dirty`) was also inspected through a separate native
720 × 540 window. Its source executable SHA-256 was
`545f904d2cb99b83e6b23d2d57d76b6e5618b6df6a717aadc93c9c55e7cab971`.
The bundle was copied to a temporary directory, assigned a separate test bundle
identifier and signed ad hoc to distinguish it from an already-open workspace.
Only that isolated preview received pointer, wheel and keyboard actions.

The import preview fit the minimum window and cancellation restored the mapping
view. The preset menu stayed anchored, scrolled to its last entry without moving
the settings body, and outside-click dismissed it without activating the
underlying Show layout button. The Manage menu remained inside the viewport;
Escape dismissed it and Done closed the preview with exit status 0. Earlier
archived captures remain evidence of the unchanged visual design; these fresh
observations did not replace them or promote hardware compatibility status.

## Final pre-commit review — 2026-10-11

Reviewed the full pending controller/SDK/settings change set: portable format
validation, interpreter permissions and resource limits, session retirement,
mixed-source hold ownership, asynchronous HID completion, timer ordering,
recording/replay, import/edit failure behavior and responsive settings controls.
No further blocking functional issue was identified within the reviewed scope.
Corrected the pre-existing module ordering in `mixless-protocol/src/lib.rs`
reported by workspace rustfmt. Added explicit filenames to 18 existing native
capture records, preserving their dates, dimensions and source build identity.

| Check | Result |
| --- | --- |
| Workspace tests excluding desktop | **432 passed, 24 ignored** |
| Desktop / GPUI settings tests | **124 passed, 6 ignored** |
| Controller tools tests | **54 library tests and 1 schema/tool test passed, 4 ignored** |
| Independent C SDK compile/package/replay | **8 frames and 8 assertions passed** |
| Virtual CoreMIDI tests | **3 passed** |
| Controller Clippy (`-D clippy::correctness`) | Passed; existing style/deprecation warnings remain |
| Workspace formatting and diff whitespace | Passed |
| Bilingual content | **24 pages, 124 search records, no errors or warnings** |
| Development bundle | Built successfully at **2026-10-10T19:12:11Z** |

The virtual tests used the freshly built controller test executable from the
successful tools run. A redundant Cargo invocation was cancelled while waiting
for another workspace's shared package-cache lock; no other workspace process
was stopped. CoreMIDI callback-to-handler timings observed in this run were
p50 **61 µs**, p95 **111 µs**, p99 **173 µs**. Other workspace builds were active,
so these are observations under background load, not a new isolated benchmark.
The prior isolated Wasm measurements remain historical evidence.

All 32 new native screenshot files have capture records. Current controller
layout, buttons, indicators and responsive details are covered in
[Settings UI review](../../apps/desktop/src/preferences/VERIFICATION.md),
including native mouse checks at 940 × 820 and 720 × 540. No UI behavior changed
during this final review. These checks do not establish physical HID/USB
compatibility, firmware behavior or controller audio routing. Remote CI is a
separate check after pushing; this local record does not claim its outcome.
