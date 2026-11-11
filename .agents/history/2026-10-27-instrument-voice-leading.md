# Polyphonic instrument notes and the BEST regression

The earlier blanket withdrawal of note-derived fades was not supported by the
causal evidence. This change restores those candidates, adds pitch/time voice
tracking, fixes double-counting in cyclic order evaluation, and corrects two
observer references. ORDER_VERSION is 18; analysis v26 and stem v2 are unchanged.
No audio callback, persisted analysis field, migration or formal-library write
is involved. Existing unrelated work is retained.

## Causal investigation

Artifacts are in `target/automix-voice-leading-20261027/`. The current consistent
read-only database backup matches the prior baseline's tracks, analyses and
playlist occurrences. All 76 source recording hashes were verified again.
These recordings are development/regression material, not an independent holdout.

Comparing all 182 directed cold DnB pairs between the v17 baseline and the
rest-only experiment found exactly one changed pair: Take Control → Power.
Its score improved from 0.51950 to 0.55615. Power has a measured, note-free
instrumental rest at approximately 9.849–10.769 source seconds. The new
instrument reveal crosses its audible range at approximately 9.884–10.423 s,
inside that rest; the next attack is preserved. The new 8-bar StemSwap starts
Power at 0.035 s, whereas the old 16-bar PhraseBlend started at 13.828 s.
This is positive evidence for that candidate, not proof of subjective quality.

The initial tour and first simulated repair round were identical. The improved
edge changed a later repair path. `order::chain` simulated the first departure
both cold and again after completing the loop, then added both costs. Opening
one edge therefore gave it twice the weight of the others.

| Rest-only repair round | Old double-counted total | Each cycle edge counted once |
| --- | ---: | ---: |
| 1 | 10.804032 | 10.131945 |
| 2 | 10.809133 | 10.292559 |
| 3 | 10.750714 | 10.206376 |

The old code chose round 3; the corrected code chooses round 1. The closing
context now replaces the cold first cost. Cold-start safety checks remain;
failed/rushed edges are deduplicated before feedback. A regression demonstrates
the erroneous rotation advantage and the corrected arithmetic. This does not
claim every rotation has identical inherited playback state.

The newly exposed In The Echo → Underwater hard cut is identical to the old
planner when given the same preceding plan, entry, rate and pitch. At the
84.168 s exit there is no active vocal; the incoming point is a rhythmic
attack around 11.07 s. Its abrupt-control flag remains a listening review item.
It is not evidence that the new note detector manufactured a vocal cut.

## Pitch/time voice tracking

`mixless_protocol::instrument_voices` groups existing combined-instrument notes
into trajectories. It needs no additional separator or model inference.

- Nearby same-pitch fragments without a new measured attack remain joined.
- Approximately simultaneous attacks are assigned jointly to preceding voices
  using minimum-cost matching, with pitch distance, temporal distance and a
  bounded previous-interval prediction. Held overlapping notes stay separate.
- Voices may cross registers. Register is a duration/confidence-weighted MIDI
  centre, not an instrument label or an assertion that the highest voice leads.
- Close alternative assignments are retained as local ambiguity evidence.
- A voice can nominate a repeated cycle only with pitch, rhythm, articulation
  and contour recurrence across three cycles. Every other active voice must
  independently corroborate that cycle. A dense bass ostinato cannot outvote a
  sparse evolving upper voice, and a held crossing part blocks the shared fade.

This remains an approximate symbolic grouping, not semantic transcription of
every melodic sentence. Missing/ambiguous evidence adds no new permission; it
does not remove existing structural candidates. Structural and note candidates
have separate bounded budgets, so additional notes cannot crowd out the old
search before fitting. Gradual audible instrument fades retain the original
0.75-local-bar requirement; an acoustically quiet reveal can still occur inside
a shorter measured rest. Instrument evidence never authorizes a whole-track
hard cut. Existing vocal, harmonic, peak and explicit-cue checks remain final.

The corpus produces 140 supported rests and two corroborated motif boundaries,
both in Feel This Good (about 146.479 and 149.306 s). This is candidate coverage,
not measured detector accuracy. Brain Power's relative low-energy window still
has no newly confirmed all-voice motif exit; the implementation does not force
one from a bass pattern. Tests cover bass plus upper line, held pads, crossing
voices, sparse evolving melody, joint-assignment ambiguity, transpose, speed,
gain and equal-onset storage order, alongside existing note/clock counterexamples.

## Metric correction and the Electronic comparison

Observer v6 incorrectly used the louder raw A/B foreground or peak as its
reference even when B was fully muted. It could report loss while A remained
unchanged. Observer v7 instead compares heard foreground/energy with A's natural
continuation. Incoming phrase damage and simultaneous foreground competition
remain separate measurements. Synthetic counterexamples verify that silent
preloading causes no loss but removing an audible outgoing part still does.

All old and new plans were re-observed with the same v7 code. Original v6 output
is retained under `final/`; final v7 output is under `observer-v7/`. Lower values
across observer versions are measurement corrections, not planner improvements.

For You, Always → I'm With You explains the fixed Electronic change. Adding the
same bounded foreground-loss preference to dry cuts changed the lookahead choice:
the old cut scored 0.47336; its missing first-bar foreground costs 0.18. A long
StemSwap scores 0.55306 but also incurs the existing following-tempo preference,
so it had previously lost to the unpenalized cut. This is not a pruning defect.

Its 88 nominal bars include preparation and residual playback, with only about
4.85 seconds of observed simultaneous audio. Both arrangements remove the
outgoing tonal layer around source 134 s. The v6 gap increase of 4.15 s per
changed occurrence shrinks to 1.0 s under v7; some exposure remains because a
longer scheduled window observes later material. Zero-bar cuts have no overlap
gap exposure, so that number alone cannot rank their arrivals.

An additional comparison aligns source 134.77695 s and follows each plan's actual
carried clock. Foreground deficit exceeding 6 dB after 1/4/8 local source bars
is 0.671/1.100/1.200 s for the old cut, versus 0.108/0.323/0.323 s for the new
arrangement. This supports retaining the candidate for listening rather than
rejecting it from overlap-only gap counts. `final/common-context.py` reproduces
that diagnostic. The dry-cut preference is bounded, uses existing reference
power and the existing 6 dB allowance, and does not penalize natural EOF/release,
unknown evidence, loops or effect tails.

## Whole-library results and remaining review

The audit contains 684 measured transitions: four playlists, three cycles of
ORDER, frozen BEST and recomputed BEST, plus successor context. State includes
the actual preceding entry, staging position, rate and pitch. All ORDER playback
plans remain equivalent; three score-only changes occur in the 29-track set.
Frozen BEST remains identical except the Electronic case described above.

Fresh BEST, using observer v7, over three cycles:

| Playlist | Foreground gap seconds | Abrupt tonal-control flags | Vocal flags at default grouping |
| --- | ---: | ---: | ---: |
| Trance, 11 tracks | 7.35 → 7.35 | 1 → 1 | 0 → 0 |
| Electronic, 22 tracks | 15.90 → 6.30 | 12 → 18 | 0 → 0 |
| 29-track playlist | 12.45 → 12.45 | 27 → 27 | 0 → 0 |
| DnB, 14 tracks | 42.45 → 37.95 | 3 → 6 | 3 → 3 |

These are tradeoffs, not a whole-library listening pass:

- Trance's >3 dB energy-deficit exposure rises 2.70 → 15.60 s, although its worst
  deficit falls 5.78 → 4.61 dB and there is no >6 dB exposure. Mean absolute
  carried pitch rises 0.824 → 1.256 semitones. Take You Home → Who Am I Now and
  Moments → Day Cycle need listening review; the existing harmonic proxy improves.
- Electronic has more mild energy dips, drum-activity changes and foreground
  overlap. >6 dB deficit falls 6.45 → 1.80 s and >9 dB falls 0.75 → 0 s, while
  >3 dB rises 18.70 → 42.27 s. More mild exposure is not automatically preferable
  to fewer deep dips. Known harmonic-conflict exposure falls, but unknown
  harmonic coverage rises and is retained as an alert.
- At the broader three-pulse vocal grouping, Electronic flags rise 3 → 6.
  Found You → Spiritbond is the additional pair: the outgoing voice is removed
  in a low-activity gap around 110–111 s before later activity. One/two-pulse
  grouping sees separate phrases; three-pulse grouping joins them. Without
  lyric/listening evidence this remains unresolved, with its own render.
- DnB's added hard cut is the unchanged In The Echo adjacency described above;
  rhythm/harmony exposure does not increase. Existing default vocal flags remain.
  Tail-short counts are descriptors, not the optimization target.

## Validation and delivery

Seven real audio renders use forced audited plans and actual cached stems when
required, including the changed Power pair, the Electronic before/after pair,
the DnB hard cut, a tempo-change case and Found You → Spiritbond. Forced plans
match at f32 precision; output has zero nonfinite or clipped samples. Each clip
includes 20 seconds after the plan for arrival review. These are render checks,
not by-ear acceptance or physical-device playback validation.

929 workspace tests pass, including 233 desktop tests, after excluding one
native HID discovery test that also fails alone with a channel timeout. The
unfiltered workspace therefore does not pass. The 62 observer tests pass.
Changed Rust formatting and `git diff --check` pass. `./dev.sh --build` produces
the dev 0.1.0-beta.5 build-1 app bundle. No app installation/restart, commit,
push or release was performed.

Detailed inputs, hashes, cold-pair/repair-round traces, plans, per-pair reports,
render manifests and checks are retained in the task artifact directory. The
final two Rust optimizations only skip impossible <9-note three-cycle searches
and avoid constructing note scope when stem playback is disabled; the musical
audit binary predates those equivalent fast paths. This record supersedes the
earlier conclusion that polyphonic input or aggregate regression justified
withdrawing all note-derived fades.
