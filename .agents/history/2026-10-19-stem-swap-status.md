# Stem swap status and classification

The reported screenshot showed the incoming deck with all four stems and its
channel closed while the toolbar already said `Stem swap`. The toolbar used the
whole plan's technique during cueing and silent playback. The fitted composition
also classified every non-vocal-carry schedule as `StemSwap`, including schedules
whose audible parts all followed the same envelope.

The toolbar now shows the scheduled countdown, `Waiting` while the incoming dry
channel is closed, the technique during the exchange, and `Finishing` after the
outgoing dry channel closes. Pause takes precedence. The tooltip retains the
planned technique and identifies the deck waiting to enter. Narrow layouts keep
these states visible and fit the complete master controls.

Composition descriptions inspect the fitted envelopes and measured part power.
They distinguish independent parts, whole-track fades and cuts, and explicitly
describe a silent incoming lead-in. Moving a negligible vocal lane cannot make a
whole-track fade a stem swap or vocal carry. The relative power floor is the
existing -40 dB floor used by composition vocal evidence; it only affects labels.
No source times, control curves, candidate scores or ordering weights changed.
`ORDER_VERSION` stays unchanged because Best order results are unaffected.

## Evidence

- Recomputed the reported `Don't you want me (feat. Such)` to `pineapple spaceship`
  pair from the historical analysis snapshot. This is a reproduction using that
  evidence, not a recording of the original screenshot's running engine. The
  36-bar plan prepares tempo for 12 bars, holds the incoming stems closed until
  bar 28, then exchanges parts. Its complete technique remains `StemSwap`.
- Compared the previous composition renderer with the final implementation on
  identical inputs: the reported pair and 72 chained handoffs across five current
  playlists. Source windows, all audio lanes, score and other non-description
  fields are identical. No technique labels changed in those real plans; the
  false-classification cases are covered by controlled regressions.
- Current library evidence came from a SQLite backup. Its 77 analysis payloads
  span versions 15, 20 and 21; this work did not reanalyze or modify the live
  library. Generated input, plans and comparison records are under the ignored
  `target/stem-swap-fix-20261019/` directory.
- `cargo test --locked --workspace --exclude mixless-desktop`: 584 passed,
  28 ignored. AutoMix UI tests: 7 passed, including both deck directions,
  lifecycle states, tooltip updates and complete master-control bounds at
  1180, 1280 and 1440 pixels. Composition coverage includes synchronized fades,
  a cut, silent vocals, a genuine exchange after a silent lead-in and gain scaling.
- `./dev.sh --build`, changed-file rustfmt checks and `git diff --check` passed.
- Native GPUI previews were inspected on macOS 27.0.1 at 1180 and 1440 logical
  pixels (2360 and 2880 pixel Retina captures), using production toolbar views
  with fixed snapshots and an isolated offline data directory. These verify
  rendering, not live playback, physical audio routing or listening quality.
  Preview processes were stopped after inspection.
- Inspected preview and development bundle executable SHA-256:
  `37521507e0416c3e9659c218973f882d26875f14c57bd240ce567214d3180177`.
  Build: `0.1.0-beta.4`, dev, `v0.1.0-beta.4-19-gd0f700c-dirty`,
  `2026-10-18T19:32:36Z`. The shared checkout contains other pending work.
