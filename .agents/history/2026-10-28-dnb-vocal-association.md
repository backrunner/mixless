# DnB vocal association and terminal handoffs

The narrower vocal-association improvement is retained for the user's later
listening. The user explicitly asked to keep the improvement rather than restore
the known-bad starting behavior merely because listening acceptance is pending.
`ORDER_VERSION` is 19. Claude's existing uncommitted work is preserved.

## Mechanism

`vocal_activity` is calculated from separated-stem RMS and relative energy; it
is not a calibrated lyric classifier. Remote faint observations on either side
of a rest cannot alone prove that the same sung line continues across it.

The planner uses the existing local 9 dB body floor to grade that association.
Actual activity at the queried boundary retains the old protection. Supported
held notes retain their body and tail protection. A separate faint event's own
note does not prove its association with another event across the rest. Direct
active-frame fades, harmonic/peak constraints and explicit cue authority remain
unchanged. Uncertainty still contributes to ranking.

No title, track ID or recorded case time is used in production rules. Tests cover
remote notes, real quiet vocals, supported tails, gain, source shift, speed,
half/double grids, true onsets/releases and the relative floor's neighbourhood.
These transformations operate on stored observations, not redecoded audio.

## Real-library evidence

Artifacts: `target/automix-dnb-continuation-20261028/report.md`, with reproducible
scripts, patches, frozen binaries, plans, observer reports and before/after audio.
The consistent current-library copy contains four playlists and 76 verified
BLAKE3 audio identities. Playlist analysis is v26, stems v2 and observer v7.
All four fixed-order, frozen-BEST and fresh-BEST comparisons include a complete
cycle and successor context. These recordings are regression material.

In the reported five-pair sequence, Divided Sky -> Flicker changes from a
terminal 1-bar fallback at 201.4 seconds to a 4-bar StemSwap at 176.6 seconds.
Flicker enters at 35.9 seconds with a +3-semitone shift. Flicker -> Lost & Found
changes from a terminal cut at 291.1 seconds to a planned 1-bar cut at 135.2
seconds. Two terminal short handoffs remain. In the full fixed DnB cycle,
Take Control -> We Are The Energy also becomes a 28-bar PhraseBlend; nominal
terminal short handoffs fall from 8 to 5.

These are planning changes, not listening acceptance. Fixed DnB adds two vocal
grouping flags and foreground-gap exposure rises from 1.10 to 4.65 seconds.
Fresh DnB BEST's heard-participation tail descriptor falls from 4 to 3, while
vocal grouping flags rise from 0 to 1 and foreground-gap exposure rises from
2.00 to 6.95 seconds. Other playlists also retain review flags. The user was
unable to audition during this review; keep the flagged entrances and their
renders available for later feedback.

Distinct-rest candidate selection and independently delayed vocal arrival were
also tried. Neither changed the reported five-pair outcome under the original
hard vocal protection. They are artifact-only experiments, not retained changes.

## Validation boundary

The retained implementation passes 220 planner tests, including three new vocal
association regressions. The full non-desktop workspace passes 700 tests with 28 explicit ignores,
and the development app builds after restoring the retained implementation. Thirteen
forced-plan offline renders use real cached stems where required and include a
mixed-tempo pair. Their plans match at f32 precision, with no nonfinite or clipped
samples. Environment: macOS 27.0.1, offline 48 kHz stereo, no physical device.
No installation, playback restart, commit, push or release is included.
