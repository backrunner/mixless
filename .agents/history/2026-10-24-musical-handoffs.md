# Musical handoffs, spectral arrivals and set intensity

This follows [the stem-exchange audit](2026-10-24-stem-exchange.md). That document
records the earlier isolated change; this follow-up also changes analysis and
ordering semantics. Neither report claims human listening acceptance.

## Implemented behavior

- StemSwap requires both sources to retain audible nonempty proper subsets for
  a full master bar. A full-track fade or one-sided selection remains PhraseBlend.
- Stem composition, native stem bridges, full-band bridges and long blend
  choreography factor their intended audible envelopes through the engine's
  square-root equal-power crossfader. The outgoing channel closes only after
  routing reaches the receiving side. EQ compensation now uses that same
  physical law, rather than a sine/cosine approximation in these paths.
- The paused incoming channel is primed to its planned fader level, so launching
  a boundary cut does not also start an unnecessary channel-level ramp from zero.
- Host-only gain prediction measures each deck's retained stem PCM, EQ, filter,
  manual gain, channel level and crossfader share separately. The weaker routed
  side has a smaller correction budget. It does not invert intentional fader
  attenuation or amplify a closed side. Both sides may still receive the same
  correction when their independent measured deficits justify it.
- A late entry into an adjacent Drop/Chorus run owes the remaining continuous
  peak, instead of allowing a single short internal Drop unit to satisfy it.
- Hard boundary cuts do not land in the last build phrase or its one-bar pickup.
  Explicit user cues retain authority; real overlapping transitions can introduce
  that build while the outgoing source is still present.
- The shared musical-energy descriptor combines source-relative RMS, spectral
  body and rhythmic flux activity. Sub-150 Hz power has weight 0.2; mid/high bands
  retain unit weight. Flux occupancy is measured per second, not inferred from
  BPM. The track statistic uses the upper quartile of active bars; per-bar and
  per-section values are available through `energy-audit`. It is a coarse
  engineering descriptor, not LUFS or a calibrated perceptual-energy scale.
- Best order uses that intensity in pair ranking and ordering. For sets up to
  40 tracks, it additionally simulates carried entry position, rate and key
  through the complete loop. Up to four bounded repair rounds penalize edges
  that need emergency handoffs in the actual chain. The best verified round is
  retained. Large-library ordering retains its previous bounded search budget.
- The tempo search retains an already phase-supported grid. A weak fit can search
  peaks of the multi-lag comb, rather than only aliases of the strongest single
  autocorrelation lag. Phase contrast against displaced controls validates the
  extra candidates. A Brain Power counterexample prevented replacing a reliable
  85.02 BPM interpretation with a dotted alias.
- Opening-drop refinement requires a measured low-frequency/rhythm interruption,
  a restart and sustained upper-band expansion. Bass mutes, isolated impacts and bright
  sustained noise are negative tests. The search is bounded to an opening run;
  it is not a universal drop classifier or a guarantee for every arrangement.
- Initial UI key notation converted Camelot storage to Open Key, including wheel origin
  (superseded by the user-requested Camelot A/B restoration in
  `2026-10-24-key-comparison.md`):
  8A becomes 1m, 8B becomes 1d. The deck badge, waveform badge and library agree;
  transposition happens before display conversion and semantic colors are retained.
  `d` means major and `m` means minor, as documented in the
  [Traktor manual](https://docs.native-instruments.com/ni-tech-manuals/traktor-pro-manual/en/global-concepts).
- Analysis version is 24 and order version is 4. The desktop reanalyzes mismatched
  caches and replaces automatic cues through its existing preservation path;
  explicit user cues are not rewritten. The live library was not changed by this audit.

## Measured recordings and coordinates

A consistent SQLite backup captured current library state. All 53 available
recordings were fully reanalyzed and their full-file BLAKE3 hashes match the
stored identities (53/53). Twenty-five older analyzed entries had unavailable
source files. Their stored measurements remain regression inputs for plan
checks, not newly decoded evidence. No source audio, personal databases or file
paths are included in the repository fixtures.

| Reported issue | Before | After |
| --- | --- | --- |
| Aeipathy global tempo | 102.402 BPM | 128.000 BPM |
| Spiritbond first drop | 25.631 s | 52.832 s |
| Ice Shards first drop | 80.035 s | 96.034 s |
| Offset in captured sequence | Enters 39.107 s, leaves 61.049 s | Enters 15.794 s, leaves 140.587 s |
| Dreams to Don't trust me | 0.160 s of simultaneous selected stems in original failure | 6.384 s in current chained render |

The Aeipathy clock retains measured local tempo changes and weak-confidence
segments. A corrected global label is not a claim that the whole recording has
one perfectly regular tempo. Across the 53 decoded files, this is the only
non-octave global BPM change exceeding one percent.

For You, Always active intensity is -4.096, versus -5.390 for rldwide To ge, in the
model's relative dB-like units. Exact section descriptors and all bar values are
saved in `target/musical-audit-20261024/energy-profile.json`.

The final Best sequence is:

Offset → For You, Always → Bloom → rldwide To ge → POWER-UP! → Back Again → Indigo Horizon → Easier to Love You - Vertile Remix → Still Waiting For You → Dreams → Spiritbond → Don't trust me → Found You → Eroge → Still Here → Aeipathy → Where Do We Go → I'm With You → Fire Away (feat. Slayyyter) - Frost Children Remix → Ice Shards → Poison → wHERE the rhythm goes

rldwide To ge is between Bloom and POWER-UP!, rather than after For You, Always.
The final chained Best replay has zero emergency fallbacks. The existing five
playlist orders and captured Best sequence also pass source bounds, carried
rate/key and nonoverlapping schedule checks. The MUG regression still has two
emergency fallbacks. The 136 checked plans include intermediate and final Best
runs; they are not 136 distinct recordings or independently judged transitions.

In the captured old sequence, wHERE the rhythm goes to Ice Shards now chooses an
earlier breakdown at 52.837 s, not a claimed drop at 80 s. In the new Best order,
Fire Away introduces Ice Shards with an actual overlapping part arrangement,
78.435–116.832 s, spanning the measured 96.034 s drop. Thus the analysis correction
does not mean every legal transition must start exactly on the drop.

## Traktor comparison

Traktor Pro 4 was launched and used to analyze a 22-track comparison playlist
containing copies of the recordings. Its exported NML is retained under the
ignored audit directory. Comparison uses the newly analyzed `MUSICAL_KEY` value;
`INFO KEY` can retain imported tags and is not independent analysis evidence.

All 22 BPM results now agree within 1 BPM after equivalent half/double-time
interpretations. Eleven of 22 musical keys agree exactly. The remaining results
are listed below; neither estimator is treated as annotated musical ground truth.

| Track | mixless | Traktor |
| --- | --- | --- |
| Bloom | C major | D minor |
| Offset | C# major | D# minor |
| wHERE the rhythm goes | G major | G minor |
| Ice Shards | B minor | E minor |
| Still Here | A# major | D# major |
| Dreams | A minor | D minor |
| Don't trust me | C# major | F# major |
| Where Do We Go | G# major | C# major |
| POWER-UP! | A major | A minor |
| Found You | D major | E minor |
| Eroge | F minor | G# major |

Key disagreements need independent tonal/listening evidence; this change does
not retune the key estimator to copy Traktor's answers.

## Validation and limits

- `cargo test --locked --workspace --exclude mixless-desktop -- --test-threads=1`:
  603 passed, 28 ignored. Parallel runs encountered two native HID discovery
  timeouts; those tests passed in isolation and in the full serial acceptance.
- Seven AutoMix status tests and one sounding-key UI test pass.
- `./dev.sh --build` succeeds. The local development bundle is beta.5 dev, build 1.
- Eleven after-change pair renders use aligned cached stems through the actual
  offline engine at 48 kHz. They finish at the expected incoming rate/pitch, with
  finite samples and peaks below full scale. The largest peak is 0.5198.
- Reported-pair telemetry records different per-deck AUTO gains during overlap;
  no observed intermediate-crossfader snapshot has an already closed outgoing
  level while that source is playing. Ten-millisecond RMS checks of the seven
  reported after-renders find no near-silent window below -70 dBFS. These checks
  do not establish equal perceived intensity or prove the absence of every
  musical loudness dip.
- Controlled tests cover false StemSwap labels, audible subsets, deferred vocal
  entry, per-side EQ/stem/fader compensation, routing reconstruction, gain/time
  translation invariance of the descriptor, a syncopated tempo counterexample,
  spectral drop arrivals, bass mutes, impacts, sustained bright noise, full prior
  peaks, and late entries into continuous peaks. The measured recordings were
  used for development/regression and are not an untouched holdout corpus.
- macOS 27.0.1 (26A434). Audio validation is offline; no hardware listening claim.
  The Mac locked before the final native UI pass. The earlier native stem-toolbar
  capture remains valid for that earlier stage, but is not evidence for the new
  key display. The tool still reported a locked Mac on the final attempt.
- No commit, push, release or live-library replacement was performed. The new
  development bundle was built but not launched to replace the user's session.

Build SHA256: `26631d34542086fba2830009dff7c0c90ab8e199f4625e7b5d0ed087f4ebcbad`.
Build revision: `v0.1.0-beta.5-1-g9651628-dirty` (working tree changes included).
All generated evidence is in ignored `target/musical-audit-20261024/`.
