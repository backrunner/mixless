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

Do not commit credentials, personal libraries, acquired audio, build artifacts,
or generated databases. Contributions are under Apache-2.0; preserve third-party
license notices. Spotify supplies playlist metadata, not decoded mixing audio.
