# Rhythmic alignment, instrument entrances and percussion releases

## Evidence and scope

The development cases are Particle Arts → EON BREAK (both available Particle Arts recordings), ShadyShady → Platinum, Platinum → Luolimasi, and Reborn's earlier breakdown entrance. This extends the layered-mixing work at `8ab02cc` and preserves the previous vocal, peak-energy, harmonic and solo-retuning guards.

A fresh SQLite backup captured the library without modifying it. All 77 source content hashes match. The snapshot contains 75 version-20 analyses and two version-15 analyses; all 77 were decoded and reanalyzed to version 21 in a separate copy. Existing source-aligned stem measurements were attached once. Automatic cues were refreshed only in the copy; manual cues retain their authority. The reported recordings have version-2 stem measurements and cached four-part PCM.

Local artifacts are under ignored `target/rhythm-cue-review-20261017/`. Acceptance data uses `accepted.db` and the `accepted-` prefix. The `trial`, `final`, `validated`, `fractional-probe` and `contrast-probe` artifacts are intermediate experiments, not acceptance results. Original source paths, databases, measurements and PCM are not committed. These recordings and playlists have been inspected during development; they are regression data, not independent holdouts.

## Analysis corrections

- The old 180 BPM ceiling excluded Platinum's approximately 182 BPM clock and selected a 121.35 BPM dotted alias. The search now covers 70–240 BPM. Each harmonic autocorrelation uses its own interpolated fractional lag: rounding the fundamental before multiplying it penalized valid fractional periods. Candidate onset folding subtracts the uniform-phase background and weights the full comb support, so a faster grid does not win merely because it has more attack windows. The initial range-only experiment exposed unrelated dotted aliases; that experiment was rejected.
- EON BREAK and Particle Arts contain small local period changes that matter over a long passage. Overlapping windows search within 4% of the coarse period. Three successive windows must agree on period and phase, and a sustained correction must exceed 0.3%. Anchors retain beat count and reject half-beat accent flips. Interpolated spans recompute their own confidence; they cannot borrow confidence from distant drums. Silence, diffuse flux and a brief fill cannot retime a recording.
- `rhythm::supports_grid` now applies the same 30 ms low-band onset measurement lag already used by the analyzer's alignment calculation. This is compensation for the analysis window, not an intentional musical phase offset.
- `percussion_release` is shared by automatic cue and entry-region consumers. It requires at least two local bars, 95% stem-frame coverage, an audible preceding kit, at least 6 dB sustained kit retreat and a continuing instrument bed. Both halves must support the withdrawal. Bass-only mutes, silence, missing evidence and the end of the file do not qualify. A synth bed can remain loud while the drums withdraw; mixture novelty alone must not discard that entrance.

Reborn's measured breakdown already began at **148.896 s**, but its mixture novelty was below the old Hot-cue threshold. The final automatic set now includes that entrance, **14.884 s / eight local bars before 163.780 s**. It also includes a separate 29.823 s structural cue; pad numbering can therefore change. This is an added source-time entrance, not a claim that a particular old pad index is always moved.

The wider tempo range can choose equivalent half/double representations: ShadyShady is represented near 200 BPM rather than 100 BPM and EON BREAK near 90 BPM rather than 180 BPM. The planner uses the corresponding metrical mapping. This does not establish canonical score notation. Aeipathy remains ambiguous: its new global estimate is 102.4 BPM with median pulse confidence about 0.10; the old 136 BPM analysis also had weak local evidence. No accurate-tempo claim is made for that recording, and weak evidence does not authorize a synchronized blend.

## Planning changes

1. Select the closest original, half or double metrical relation before checking speed budgets. Shared preparation first intersects both decks' absolute ±8% ranges and movement from both carried rates. If that intersection is empty and separated parts are available, it may try ±12%; the selected plan must actually require stems. Preparation remains silent on B, respects the logarithmic 0.35%/s slope bound and passes its actual completed state forward. This absolute bound applies to this preparation path; existing direct blends retain their separately bounded movement policy.
2. A quiet introductory or recovery pickup may enter at the first reliable downbeat within its next four bars, with four measured bars still available. It must follow an evidenced section boundary and successfully fit a separated-part arrangement. It is not promoted to a phrase boundary or used to authorize an unarranged whole-track cut. Explicit IN cues still take precedence.
3. Native-clock bridges reject two audible rhythmic foregrounds even when the drum controls themselves do not overlap. Instrument and vocal onsets include arpeggios, plucks and chopped voices; instrument low-band power remains a bass proxy. The engineering veto uses normalized onset activity and a 1% contribution against the heard power. Sustained pads may remain; missing evidence cannot authorize the fit.
4. Synchronized composition also prices instrument-control movement per wall-clock second, weighted by the measured instrument power. A loud chord cannot benefit from a very short reveal merely by reducing simultaneous energy. Cross-key fits can lengthen the outgoing fade backwards and incoming fade forwards while retaining tonal separation. This is a fitting objective, not a universal hard slope limit for every instrument entrance.

Analysis version is **21**, order-cache version is **3**. All estimation and curve fitting remain on the host. DSP and callback processing are unchanged.

## Final plans and rendering

The standalone pairs use these source-time windows. Silent tempo preparation is excluded from the overlap columns. Bars follow the selected outgoing grid, so half/double representations are not interchangeable bar counts.

| Pair | A during overlap | B during overlap | Overlap | Silent preparation |
| --- | --- | --- | --- | --- |
| Particle Arts → EON BREAK | 117.920–123.468 s | 55.091–60.426 s | 4 bars / 5.55 s | none |
| Alternate Particle Arts → EON BREAK | 117.720–123.270 s | 55.091–60.426 s | 4 bars / 5.55 s | none |
| ShadyShady → Platinum | 67.689–82.089 s | 0.033–15.858 s | 12 bars / 14.65 s | 10 bars |
| Platinum → Luolimasi | 65.969–80.474 s | 15.022–27.022 s | 11 bars / 13.64 s | 30 bars |

All four are synchronized StemSwap plans. The Particle pairs now pass a reliable incoming downbeat instead of overlapping independent clocks. ShadyShady prepares rates about 0.98268 / 1.07990; its kit exchange takes roughly two seconds on B and four seconds on A. Platinum → Luolimasi prepares about 1.06384 / 0.88010; Luolimasi's instrument envelope takes about 3.91 seconds to go from 1% to 99% heard gain, instead of the old short chord reveal. In the chained case, Platinum inherits 1.07990 and needs eight preparation bars, followed by the same eleven-bar overlap at a different feasible shared clock.

Independent 512-point / 64-hop spectral flux from cached drum PCM, resampled to 11.025 kHz, was mapped through each plan's source clocks. Dominant pulse-cluster centroids differ by approximately 2.4, 2.1, 2.6 and 6.9 ms respectively. The measurement hop is 5.8 ms: these are approximate cluster observations, not sample-accurate guarantees, maximum errors of every syncopated hit, or proof of canonical downbeats. The unthresholded flux audit is `accepted-flux-phase-check.json`; an earlier sparse peak-picking audit was unsuitable for dense subdivisions and is not acceptance evidence.

Actual four-stem 48 kHz / 256-frame offline rendering completed all four baseline and four new pairs, with finite, in-range samples and expected final rate/pitch. New peaks are 0.3078, 0.2428, 0.4337 and 0.4000. Output files include four seconds of context at either end, and preparation time where applicable.

The six-track reported chain renders **745.31 s**, peak **0.47042**, with all five handoffs completed. The frozen 29-track regression sequence renders **3064.17 s**, peak **0.53640**, with all 28 handoffs completed. Actual ending source positions, rates and pitches pass the renderer's 0.03 s / 0.002 / 0.02-semitone checks. This replays the captured order; it is not a new Best-order optimization run.

Four other playlist orders cover 71 adjacent pairs. Source bounds, finite controls, completed offsets and entrance/exit staging pass across those and the frozen sequence. The broader 71 pairs change from 41 to 46 overlaps of at least four real bars and from 20 to 15 zero-bar cuts. The frozen 28 transitions change from 20 to 18 such overlaps and from six to eight cuts. Qualia → Still and Platinum → Ghost Voices lose their old long overlap in that sequence. Other shortened cases include Aeipathy → Spiritbond, I'm With You → Indigo Horizon, Where Do We Go → For You, Always, Particle Arts → Ghost Voices, EON BREAK → Ember and Feels So Right → Particle Arts. Corrected clocks, stricter rhythm evidence and changed carried states affect these results; counts alone do not establish better listening. In particular, EON BREAK → Ember still uses a conservative late handoff. Earlier Alone → Ember, Used to Be → Angelic Sphere, Don't You Want Me → Crystallized and Your voice so → Stella retain real overlaps in the continuous replay.

## Validation and limits

- Workspace tests excluding desktop: **567 passed**, 0 failed, 27 ignored.
- Desktop tests: **185 passed**, 0 failed, 7 ignored.
- `./dev.sh --build`: passed on macOS 27.0.1 (26A434), using the isolated Cargo target directory.
- Build identity: 0.1.0-beta.4 / dev, revision `v0.1.0-beta.4-13-g8ab02cc-dirty`, timestamp `2026-10-17T14:23:39Z`.
- Executable SHA256: `201cbefeee32fc376da564ac287ccbdd47a8a557cc162ac73d762cb1c5507abe`.
- Changed-file rustfmt and `git diff --check`: passed.

Counterexamples include diffuse flux, one short fill, gain and time shifts of a local tempo change, slow/fast/dense-subdivision drum signals, a quarter-beat phase error, sparse attacks, plucked foregrounds on unsynchronized decks, sustained pads, bass-only mutes, silence, missing frames, weak pickup clocks and manual IN authority. Signal-fixture transformations and decoded-audio analysis are distinguished; none establishes universal cross-genre accuracy.

No human listening, physical-device playback, native UI or real-time performance claim is made. The development bundle was built without launching or installing it. The live database is unchanged and will need normal version-21 reanalysis when this code is used. Callback budgets were not rerun because no callback or DSP implementation changed.
