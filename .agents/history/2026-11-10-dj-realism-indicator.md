# DJ practice indicator (2026-11-10)

Adds `realism` (version 3) to the offline observer. Observer measurements are
version 8, which only adds the sounding pitch of an unlocked incoming deck;
plans without that field measure as in version 7. Definitions are in
[the observer README](../../scripts/automix_eval/README.md#dj-practice-comparison).
Planner results measured with it are in
[solo tempo return and transposition limit](2026-11-10-tempo-key-return.md).

## What it reports

- `score`: 0–100 per transition and per run. Nine checks (structure, tempo,
  key, settle, exit, beat sync, harmony, vocals, energy) each earn 1 / 0.8 /
  0.5 points for the tightest of strict / default / lenient they pass. Sixty
  percent of the score is the mean of those points; forty percent goes to
  transitions with no deviation at default (half of it at lenient). Clean
  transitions score at least 88 and flawed ones keep partial credit.
- Queue allowance: tempo is judged only beyond the smallest per-deck change that
  beatmatches the pair, and clashing native keys allow one more semitone.
- `settle`: the allowance ends with the overlap. The tempo and key a track
  keeps after the plan are judged without it, except a tempo offset towards the
  next track.
- `exit`: a record played out to its tail with under six seconds of heard
  overlap is not a clean mix. A cut in the middle of a record is not charged
  for replacing the tonal parts at once, except at strict.
- `clean_rate`: the same checks as counts per level, without points.
- `dj_reference`: tempo adjustment and transposition per played track next to
  the shares Kim et al. measured in 1,557 real DJ mixes (ISMIR 2020,
  arXiv:2008.10267). No allowance, and not part of the score.
- `overlap`: heard against nominal bars, cuts and play-outs, as descriptors.

## Versions

- Version 1 counted clean transitions with no score and no queue allowance.
- Version 2 added the score and the allowance. It read 81.8 on the task-entry
  plans, because a tempo or key the pair required was free for the whole pass
  and a played-out cut counted as clean.
- Version 3 added `settle`, `exit` and the cut rule. The same plans read 68.7.
  Scores from different versions are not comparable.

## Task-entry planner under version 3

Frozen plans from [planner performance](2026-11-10-planner-performance.md): the
11-track fixed-order chain and the first three entries of three other
playlists, analysis version 26 with stems. Sixteen transitions, twenty played
tracks.

| Run | Transitions | Score | Clean (default) | Clean (lenient) | Unscored |
| --- | ---: | ---: | ---: | ---: | ---: |
| 11-track chain | 10 | 62.9 (bounds 56.6 to 66.6) | 1 | 4 | 1 |
| Electronic excerpt | 2 | 74.7 | 0 | 2 | 0 |
| MUG excerpt | 2 | 74.7 | 0 | 2 | 0 |
| DnB excerpt | 2 | 83.0 | 1 | 2 | 0 |
| Pooled | 16 | 68.7 | 2 of 15 | 10 of 15 | 1 |

Default-level deviations: settle 9, key 3, exit 2, vocals 2, energy 2,
harmony 1; tempo, beat sync and structure 0. The loop-out transition is
unscored because the observer cannot follow a looped source.

| Played tracks (20), no allowance | AutoMix | Real DJ mixes |
| --- | ---: | ---: |
| Tempo adjusted by less than 5% | 70% | 86.1% |
| Tempo adjusted by less than 10% | 85% | 94.5% |
| Tempo adjusted by less than 20% | 100% | 98.6% |
| Transposed | 40% | 2.5% |
| Transposed tracks moved by one semitone | 3 of 8 | 94.3% |

All sixteen pairs have clashing native key labels, and every pair includes a
label under 0.55 confidence, so the key allowance rests on weak labels here.
Heard overlap is 46 bars against 329 nominal bars. `automix-audit` prints its
overlap share and played-out count from nominal bars, so its summary line
overstates blending.

## Boundaries

- Point values, the clean share, the allowances and every limit are engineering
  assumptions, not calibrated against listening. `settle` was added before the
  planner work. `exit` and the cut rule were added during it, after the
  observer rated a played-out fallback cut as clean and every cut as an energy
  failure. All compared plans were rescored with the final version.
- Three runs have two transitions each; their scores are not stable estimates.
  Best order was not evaluated.
- The score does not reward overlap length. Read cuts, play-outs and heard
  bars beside it.
- No listening session, no rendered audio review and no human DJ mix of these
  playlists. The score ranks what to audition; it does not accept a mix.

## Validation

- Observer tests: 83 passed, including twenty realism tests.
- Reports are in ignored `target/automix-indicator-20261110/`. Input SHA-256
  prefixes: chain analyses `397937a3058748ce`, chain plans `9ed9d385499a4b67`.

```sh
python3 scripts/evaluate-automix.py --analyses target/planner-perf-20261110/set-3.json --plans target/planner-perf-20261110/final-set-3-plans.json --output target/automix-indicator-20261110/set-3-report.json --sensitivity
python3 -m unittest discover -s scripts/automix_eval -p 'test_*.py'
```
