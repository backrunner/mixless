# Cross-playlist entry correction — 2026-09-24

The replacement loader was publishing the selected source frame correctly, but incoming-point selection was biased toward quiet openings in two planning paths. This was missed by the earlier native replacement check, which verified candidate identity but did not reject the fading entry.

## Mechanism and change

A normal PhraseBridge cuts to B immediately. Its ranking previously averaged B’s first four bars, allowing later loud material to hide a weak fade-in in the first bar. Ranking now uses the first source bar at the entry, on B’s own local grid. BeatBlend continues to evaluate its actual overlap/handoff window.

The EOF/late-activation `short_handoff` fallback previously sorted safe entries by source time and selected the first. It now compares every safe entry with the outgoing audio using the existing phrase, energy, quiet-pair and position scoring terms. Shared energy helpers preserve the normal planner’s existing weights; this introduces no track-specific constants or requirement to skip the start.

Manual IN remains authoritative. An established intro or a consistently quiet recording may still start at the beginning. Safety gates for outgoing phrase completion and candidate replacement remain unchanged. No audio callback or DSP changes were needed.

## Real-library reproduction

A fresh SQLite backup was made for this run; source analysis version was 18. Back Again (hash prefix `blake3-cf542`) and Better Here With You (`blake3-12432`) form the primary reproduction. Let me bloom (`blake3-f6182`) and the first five Electronic tracks provide additional, already inspected regression pairs, not an independent holdout set.

| Outgoing | Incoming | Old entry, seconds | New entry, seconds |
| --- | --- | ---: | ---: |
| Back Again | Better Here With You | 0.015 | 7.842 |
| Still Waiting For You | Better Here With You | 0.015 | 7.842 |
| Don’t trust me | Better Here With You | 0.015 | 7.842 |
| Still Here | Better Here With You | 23.496 | 23.496 |
| POWER-UP! | Better Here With You | 23.496 | 23.496 |
| Don’t trust me | Let me bloom | 0.294 | 4.432 |

Four other DnB-entry pairs retained their entries, including a valid early overlapping entry. The primary reproduction’s first bar had RMS 0.057 versus a track active reference of 0.367; the following bars were substantially louder. Those later bars no longer disguise the immediate arrival.

The desktop Watch → prepare → publish integration test starts the original pair, advances A for 20 seconds, selects the new playlist, and replaces B. It verifies that the new candidate sits at frame 345840 / 44100 Hz (7.842 seconds), A continues, B stays silent until scheduled, and the handoff completes. With the existing four-stem cache attached, rendered PCM remained finite and within range; peak was 0.43190.

Electronic’s complete 19-handoff sequence also passed nonoverlap, carried-offset and exact published-plan reuse checks. Relative to the earlier run, two entry decisions changed and one following exit adjusted to its later entrance. POWER-UP!’s existing 92.759–107.306-second blend was unchanged.

## Native check and validation

Launched the new dev bundle on macOS 27.0 (26A428), external-headphone output, master level 80%. Started Electronic AUTO, then selected Melodic while Back Again continued. The idle deck changed from Still Here to Better Here With You, visibly positioned past its fade-in; remaining time changed from approximately 4:07 to 4:00. The list showed the matching nonzero IN position and a complete future forecast. Paused with this cross-playlist example available for inspection.

- New controlled regressions cover normal and fallback entry selection at 90/120/175 BPM with gain scaling, legitimate initial/quiet entries and manual IN preservation. Temporarily restoring the old four-bar ranking window makes the new regression fail.
- Workspace checks: 381 passed, zero failed, 18 ignored by default.
- Desktop checks: 110 passed, zero failed, six ignored by default. The two cross-playlist audit tests and complete Electronic audit were run explicitly.
- `./dev.sh --build` and `git diff --check` passed.
- Actual native staging and real-PCM handoff execution were verified; subjective listening quality is not claimed.

Generated evidence is ignored under `target/automix-review/2026-09-24-entry/`: `before.log`, `entry-window.log`, `after.log`, `regression-before.log`, `replacement-render.log`, `set-regression.log`, test logs, and `live.log`. No source audio or database is committed.

Reproduce with the desktop ignored tests `audit_cross_playlist_entries` and `audit_cross_playlist_replacement_keeps_the_selected_entry`, setting `MIXLESS_SET_AUDIT_DB` to an absolute SQLite backup path. Set `MIXLESS_SET_AUDIT_STEMS` to the existing cache to exercise four-stem replacement/rendering. These opt-in fixtures target playlist IDs 4 → 3/2 in the measured library; production logic contains no such IDs.
