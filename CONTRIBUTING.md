# Contributing

Mixless currently targets macOS. Install current stable Rust and full Xcode
with the Metal compiler, then run `./dev.sh --check` and `./dev.sh`.

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

## Module boundaries

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
