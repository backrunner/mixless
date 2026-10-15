# Contributing

mixless currently targets macOS. Install current stable Rust and full Xcode
with the Metal compiler and CMake (for the bundled FLAC encoder), then run
`./dev.sh --check` and `./dev.sh`.

`dev.sh` builds and refreshes `target/app/Mixless.app` with the workspace
version and the `dev` channel, then launches that bundle. `--build` refreshes
it without launching. Development builds do not install public updates.
Keep the workspace version ahead of the latest published beta when starting
the next beta; use `cargo run -p mixless-tools -- set-version VERSION` followed
by `cargo update --workspace`. Bundle version and channel are read from the
executable's `--build-info`; explicit packaging overrides must match it.

Enable the repository commit-message hook after cloning:

```sh
git config --local core.hooksPath .githooks
```

Use your own author identity. Maintainer commits use
`BackRunner <dev@backrunner.top>` (configured locally, never globally).
Use Conventional Commits: `type(scope): description`, for example
`fix(desktop): refresh the jog platter on display frames`. Allowed types are
feat, fix, docs, style, refactor, perf, test, build, ci, chore and revert.
Use `!` for breaking changes and describe the migration in the commit body.
Keep commits focused, with an imperative description.

Before submitting a change:

```sh
cargo test --locked --workspace --exclude mixless-desktop
./dev.sh --build
```

Format changed Rust files with rustfmt. Explain the behavior change and relevant
validation in your pull request. For UI or DSP work, report the macOS version,
audio device and any listening or frame-time measurements; automated tests do
not establish listening quality or smooth animation by themselves. Network and
external-tool tests may be ignored by default; run relevant ones explicitly.

## UI and product copy

These requirements apply to the native app, settings, menus, select options,
dialogs, website and product documentation, in every language.

- Avoid AI slop: generic promotional copy, vague feature claims, repeated help
  text and decorative UI that does not help the user complete a task. Use concrete
  labels and short, direct instructions. Prefer a restrained, modern tool UI with
  clear hierarchy, consistent spacing and recognizable controls. Do not turn
  ordinary settings into nested cards, badges or oversized rounded panels.
- Do not separate inline text with dots or bullets such as `·`, `•` or `●`.
  Use labeled values, separate rows, parentheses or a short sentence instead.
  For example, show `Note 11 (Ch 1)` rather than `Ch 1 · Note 11`; show connection
  status and the active preset in separate fields rather than joining them with
  a dot. This rule concerns text separators, not normal punctuation, semantic
  list markers, status indicators or the dot in the `mixless.` wordmark.
- Keep the primary view concise. Selecting a device or preset must not reveal
  long protocol descriptions, internal identifiers or raw input data by default.
  Put those details behind an explicit disclosure. Keep actionable errors and
  information needed to make a decision visible; do not hide required warnings
  just to shorten the page.
- Select options should identify the choice without repeating descriptions or
  badges on every row. Add only the information needed to distinguish ambiguous
  choices. Buttons must name their action; segmented controls must show a clear
  selected state and keep their associated content in sync.
- Menus must stay anchored to their trigger, fit the viewport and scroll when
  needed. Hovering across a trigger and its menu must not cause flicker or
  dismissal. Check outside-click behavior, keyboard navigation and small-window
  layouts alongside the visual design.

Review UI changes against these rules using actual rendered views, including
empty, selected, expanded and error states where relevant. Follow the
[mixless brand guide](.agents/skills/mixless-brand/SKILL.md) for typography,
colors and screenshot evidence. A successful build alone is not a UI review.

## Module boundaries

Controller presets and adapters follow the
[controller SDK](crates/mixless-controllers/README.md) and
[independent ABI](crates/mixless-controllers/ADAPTER_API.md). Include author and
license information, independent protocol sources, and reviewed replay fixtures.
Do not use or convert Mixxx code, scripts, mappings or test data. Keep protocol,
simulation and physical-device evidence distinct. Run the controller tool tests
and the C SDK example check when changing the format or ABI.

Split growing files by responsibility. Around 400–500 lines is a useful point
to review a module; it is not a reason to divide one coherent implementation
into numbered fragments. Keep entry modules focused on types, composition and
stable exports. Extract large view sections into named render functions, and
keep control actions separate from rendering. Private implementation details
stay private; prefer `pub(super)` for interfaces used only inside a feature.

| Area | Implementation ownership |
| --- | --- |
| `mixless-protocol/src/fx/` | Stable kind IDs and parsing in `kind.rs`, display metadata in `metadata.rs`, DSP capabilities/timing in `capabilities.rs`, persisted state and defaults in `state.rs` |
| `mixless-engine/src/effects/` | Lifecycle in `mod.rs`, sample dispatch in `process.rs`, DSP cores in `delay.rs`, `capture.rs`, `pitch.rs`, `spectral.rs` and `vocoder.rs` |
| `mixless-engine/src/engine/` | Host/device lifecycle, command validation, block mixing, deck rendering, atomic FX transfer and snapshots in separate modules; `engine.rs` defines shared state |
| `apps/desktop/src/fx/` | Edit actions in `mod.rs`, value mapping in `params.rs`, selector/editor in `editor.rs`, deck slots in `bar.rs` |
| `apps/desktop/src/views/deck/` | Header, tempo, performance controls and waveforms; `deck.rs` composes the panel |

Deck-area layout and control changes must update the MIDI target catalogue and
the Preferences MIDI wireframe in the same change. Keep the control count,
order, labels and target semantics aligned, and update the MIDI mapping tests
and user documentation when a control is added, removed, renamed or moved.

When adding an FX kind, append its stable ID and update parsing, metadata,
capabilities, defaults, DSP dispatch and the relevant editor parameters together.
Keep the dispatch exhaustive. Update [FX_CATALOGUE.md](.agents/reference/FX_CATALOGUE.md) and
[AUDIO_DSP.md](.agents/reference/AUDIO_DSP.md) with the behavior and validation boundaries.
Construct DSP buffers before playback; changing parameters or processing audio
must not allocate, perform I/O or acquire blocking locks. Module extraction must
preserve this property and must not introduce trait-object dispatch in the hot path.

Keep tests with the code they exercise: core DSP tests beside the core,
catalogue-wide tests in `effects/tests.rs`, and engine integration fixtures in
`engine/tests/` grouped by behavior. Preserve existing regression coverage when
moving code. For changes to FX or callback processing, also run the release
budgets without concurrent workloads:

```sh
cargo test --locked --release -p mixless-engine callback_budget -- --ignored --nocapture --test-threads=1
```

Do not commit credentials, personal libraries, acquired audio, build artifacts,
or generated databases. Contributions are under MPL-2.0; preserve third-party
license notices. Spotify supplies playlist metadata, not decoded mixing audio.
