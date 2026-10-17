# Adaptive stem bridges — 2026-10-17

ShadyShady → EON BREAK exposed a weakness in native-tempo bridges. The old
stem exit brought the incoming track in complete and removed outgoing drums
and bass within 1/16 of an outgoing bar, then faded its foreground. That
provided an effect tail but still changed the rhythm abruptly. On the fresh
library snapshot with the harmonic fixes at `fd9b40d`, this pair instead chose
a dry cut: unreliable harmony prevented the old tonal overlap.

## Change

`bridge/stems.rs` fits eight independent stem curves to each candidate window.
It searches incoming source downbeats and tonal exchange times, then optimizes
each deck/part's fade width against the combined measured power and competing
onsets. Fades use quintic easing, with local beat durations rather than fixed
percentages of an FX preset. Both tracks retain their own rates.

Drums and bass never overlap across unrelated clocks. Without reliable
harmony, all pitched parts change hands separately; measured remaining parts
must carry that exchange. Sung notes, incoming peaks, outgoing peak completion,
user cue ranges and source bounds remain constraints. Missing stems or a
drum-only gap cannot authorize a long bridge.

Each deck has independent band attenuation and a level ride. Band contribution
determines masking cuts; controls slew at at most 1 dB/s and return to neutral.
Channel gain compensates the engine's actual square-root crossfader law.
Positive compensation near a closed crossfader acts on silent stem envelopes;
the solver does not boost a missing musical part. Exact fade endpoints are
included in the compiled curves so interpolation cannot briefly overlap two
incompatible kits or keys.

Energy validation compares the result with the available sources at the same
instant. A breakdown's first impact is not a sustained target level. The
existing nine-dB rejection budget is retained for additional loss caused by
the arrangement. Current analysis combines bass with other instruments, so
the search uses its low band as a bass-power proxy. This is a prediction,
checked separately through real four-stem rendering, not a new four-part
measurement or a claim of perfect energy reconstruction.

All fitting runs on the host. The audio callback and protocol are unchanged.
Whole-track vocal protection and tonal separation no longer overwrite already
fitted, safe stem envelopes.

## Reproduction and evaluation

Source: consistent SQLite backup, analysis version 20, stem evidence version 2.
The reported sources' BLAKE3 hashes were verified against the stored analysis:
ShadyShady `f31f1685efb37dd1`, EON BREAK `1b07f26deada820d`.
Separator hash prefix `d05c269d0178d2a7`; note model prefix `2c3c1d144bfa61ad`.
Stored planning evidence was not reweighted.

The reported pair is the development example. The new 4-bar bridge spans
ShadyShady 161.28894–170.88928 s and EON BREAK 52.22154–61.82188 s, 9.60034 s
of wall time. Both rates stay at 1.0 (about 100 and 178 BPM). The outgoing kit
fades over about 1.2 s; incoming drums begin at 2.69354 s into the bridge.
Pitched material changes later, around 8.31370 s. The incoming bass therefore
has a different schedule from its drums. This uncertain-key pair uses separated
tonal material and no pitch shift. EQ curves differ between decks and bands.

Four complete playlist sequences passed 71 entrance/exit and carried-offset
checks. Eleven transitions used the new fitter, with durations of 4, 8 or 16
bars and distinct envelopes. Dense sampling verified nonoverlapping drums and
bass, full incoming stem restoration and neutral endpoint EQ.

Two unrelated pairs were selected after development for real-PCM evaluation;
their results were not used to adjust the solver. This is a limited evaluation
set, not evidence for every genre.

| Pair | BPM | Render including context | Peak | Stem blocks |
| --- | --- | ---: | ---: | ---: |
| ShadyShady → EON BREAK | 100 → 178 | 17.60 s | 0.4173 | 3301 |
| rldwide To ge → Still Waiting For You | 97 → 150 | 47.59 s | 0.5289 | 8924 |
| Where Do We Go → For You, Always | 135 → 175 | 22.22 s | 0.4581 | 4167 |

All three completed with finite, in-range PCM and the expected carried pitch
and rate. ShadyShady → EON BREAK → Platinum also completed both handoffs in a
354.40 s continuous render, peak 0.49273. The original live application was not
restarted. These were offline renders on macOS 27.0.1 (26A434), with no output
device; subjective listening and physical playback are not claimed.

## Checks

- Workspace: 533 passed, zero failed, 26 ignored.
- Desktop: 185 passed, zero failed, seven ignored.
- Mixplan: 134 passed, including endpoint/curve slope, incompatible-key and
  drum separation, source-dependent schedules, gain invariance, absent beds,
  missing stems and held-note counterexamples. Synthetic transformations
  operate on measured features, not re-decoded audio.
- `./dev.sh --build`, changed-file rustfmt and `git diff --check` passed.
- Bilingual docs: 24 pages, 132 search records, zero errors/warnings.

Private snapshot, source paths, plans, logs and WAVs remain ignored under
`target/tempo-bridge-review-20261017/`. The pair baseline is the current planner
at `fd9b40d`, not an assertion about the exact plan in the user's still-running
older application. The audit executable was pinned in that directory to avoid
the two crates' identically named `automix-audit` examples overwriting it during
workspace builds.
