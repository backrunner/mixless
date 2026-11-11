# Solo tempo return and transposition limit (2026-11-10)

Planner changes for arbitrary track pairs: a tempo offset is temporary, and
transposition is limited and priced. `ORDER_VERSION` is 25. Analysis is
unchanged. The engine and protocol changed only for unlocked incoming playback,
described below. Measured with observer `realism` version 3, described in
[the DJ practice indicator](2026-11-10-dj-realism-indicator.md).

## Changes

- `tempo_recovery`: after the overlap, the incoming deck glides back to its own
  tempo at the existing 0.35%/s limit (stage "Restore tempo"; 12% takes about a
  minute). It keeps part of the offset only when the following track needs a
  move in the same direction: the point where both decks would meet in the
  middle, at most 6%, never more than is already held. The glide may use half
  of what remains before the closing eight bars; a shorter remainder gets a
  partial correction. An explicit deck tempo is the reference and is not
  restored. Plans from `prepared_handoff`, explicit strategies and failed plans
  are left alone.
- Lookahead: every candidate settles its tempo before the next pair is
  forecast, so the comparison starts from the state actually reached. A
  transposition or tempo offset still carried when the plan ends is charged in
  proportion to how much of the record remains (`carried_offset`).
- Key: automatic transposition is limited to two semitones from the recorded
  key (`score::MAX_KEY_SHIFT`, previously six), for both label-based and
  measured shifts. The first semitone keeps its small cost; the second is
  priced as a last resort (`score::shift_cost`). Readable keys out of reach now
  stay a reliable conflict instead of becoming unknown, so they use the
  existing separated-part, filtered or handoff paths.
- User documentation (English and Chinese) and the planner reference describe
  the new key limit and the tempo return.

- Unlocked incoming playback (`incoming_varispeed`, `varispeed::fit`). When an
  overlap holds the incoming deck at one speed and that speed's own pitch
  (`12 * log2(rate)`) is within 10 cents of the transposition the keys need,
  the deck plays with key lock off: no time-stretch and no pitch-shift. It
  needs reliable keys, no explicit tuning on the deck, under 5 cents of drift
  across the overlap, and a pitch between half a semitone and the limit. A
  record that fits as recorded, or that a smaller locked shift reconciles,
  stays locked. The solo glide then returns all the way to the recorded speed,
  taking the pitch with it at no more than about 6 cents a second; the separate
  pitch restoration skips these plans.
- Protocol: `MixPlan::incoming_varispeed` (absent means locked),
  `MixPlan::incoming_pitch_at` and `DeckSnapshot::sounding_pitch_semitones`.
  The desktop transition bar, deck key badge and carried offset use the latter.
- Engine: the plan sets the incoming deck's key lock when it is staged and
  when it starts; the outgoing deck is always locked. At the end of the plan
  the deck is locked again if it is back at its recorded speed, and stays
  unlocked otherwise so the pitch does not jump. SKIP during an unlocked glide
  holds the speed and pitch reached.

## Results on the frozen inputs

Same four inputs as the indicator note: sixteen transitions, twenty played
tracks, fixed order, analysis version 26 with stems. "Before" is the task-entry
planner; its plans reproduce byte for byte.

| | Before | After |
| --- | ---: | ---: |
| Pooled score (15 scored, 1 unscored) | 68.7 | 85.2 |
| Clean at default | 2 | 7 |
| Clean at lenient | 10 | 15 |
| Tracks at least 5% off tempo over their pass | 6 | 2 |
| Tracks at least 10% off tempo | 3 | 0 |
| Heard time at least 10% off tempo | 11% | 0% |
| Transposed tracks | 8 | 6 |
| Tracks transposed by two semitones or more | 5 (up to 6) | 2 (both 2) |
| Heard time transposed by two semitones or more | 19% | 6% |
| Transitions with at least four heard bars | 6 of 15 | 6 of 15 |
| Heard overlap, bars | 46 | 48 |
| Cuts | 1 | 3 |
| Played out | 2 | 3 |

Default-level deviations, before and after: settle 9 and 2, key 3 and 0,
exit 2 and 3, vocals 2 and 1, harmony 1 and 2, energy 2 and 2.

Per run: 11-track chain 62.9 to 88.0, Electronic excerpt 74.7 to 85.7, MUG
excerpt 74.7 unchanged, DnB excerpt 83.0 unchanged.

What got worse or stayed poor:

- 104 → 105 was a six-bar stem swap with a three-semitone shift. It is now a
  dry cut near the end of 104 (played out).
- 102 → 101 was a two-bar blend with a three-semitone shift and is now a drop
  cut in the middle of the record. 95 → 97 is still a dry cut.
- 103 → 104 lost its six-semitone shift and blends without transposing; the
  harmony screen flags it.
- 99 → 98 and 106 → 107 use the second semitone and keep it for the pass.
  106 → 107 still has harmony and vocal deviations (score 68.7).
- No transposition was restored in solo in any run. The existing pitch
  recovery needs a quiet, unsung passage and found none.

Unlocked playback did not apply to any of the sixteen transitions: the closest
was 41 cents from agreement. With it enabled the plans are identical to the
"After" column apart from the new field. Its behaviour is covered by tests
only; it has not been heard.

Planning time did not grow: the 11-track chain took 65 s here, with other
applications running. This is one local observation, not a benchmark.

## Tried and not kept

- One-semitone limit. Score 86.0, but five play-outs instead of two and four
  heard blends instead of six: pairs that needed more became cuts at the end of
  the outgoing record, one of them a fallback. The two-semitone limit with a
  priced second semitone used it for exactly those pairs.
- No beat-matching beyond a 12% tempo gap. Score 82.3, three play-outs, four
  heard blends, and six of sixteen transitions at zero or one nominal bar. The
  planner's unsynchronized exits are mostly hard cuts, so the prepared-tempo
  stem swap stays until those exits improve. It already requires separated
  parts above 8% per deck.

## Not done

- Choosing the tempo split so that the incoming speed lands on a semitone.
  The planner only uses agreement that the least audible tempo move already
  produces.
- Smoother unsynchronized exits and longer heard overlaps. Six of fifteen
  transitions share four heard bars; that did not change.
- Best order was not re-run. `ORDER_VERSION` 25 invalidates stored orders.

## Validation

- Workspace tests excluding desktop: 743 passed, 28 ignored. Desktop tests: 233
  passed, 7 ignored. `cargo check --locked --workspace --all-targets` passed.
- New planner tests: tempo target, glide slope and source clock, partial
  correction, kept offset for a faster next track, untouched failed/explicit
  plans, planned pairs across five tempo gaps, carried-offset cost, key pairs
  within the limit, keys out of reach. Four existing tests that asserted a
  three-semitone shift now use a one-semitone pair.
- New engine test: a plan with a solo tempo glide compiles, plays to the target
  rate on either deck, and SKIP lands on the target rate.
- Unlocked playback tests. Planner: the decision rule (direction, tolerance,
  limit, fits as recorded, smaller locked shift, explicit tuning, drift, weak
  keys), a real four-bar held overlap in each case, and the glide returning
  tempo and pitch together. Engine, on either deck: staged and played
  unlocked with no KEY offset, relocked at the recorded speed, left unlocked
  off it, SKIP holding the speed reached, and an ordinary plan staying locked.
- Observer tests: 83 passed. Observer measurements are version 8: an unlocked
  deck's sounding pitch includes its speed.
- `./dev.sh --build` and the callback budgets were not run. The audio callback
  gained one comparison and one atomic store at the end of a plan. No listening
  session, rendered audio or device test. The score is a proxy.

Scratch tooling and reports are in ignored
`target/automix-indicator-20261110/` (`audit.sh PREFIX`, `summarize.py PREFIX`;
`final` is before, `final2` is after).
