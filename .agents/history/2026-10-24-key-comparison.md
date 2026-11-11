# Camelot display and four-product key comparison

Date: 2026-10-24. Status: all 22 recordings compared with Traktor, djay and VirtualDJ.

Later follow-up: [local tonality and metadata-free comparison](2026-10-24-local-tonality.md)
documents fresh Traktor and djay runs on independent untagged copies, plus
section and actual-overlap analysis. All 22 keys in each app remained unchanged.

Follow-up: [method and implementation audit](2026-10-24-key-audit.md) found
malformed MP4 integer BPM tags in the original inputs and a degenerate-chroma
key-label bug. It also rechecked source audio identity, export parsing and all
22 current Mixless analyses. The comparison counts below remain unchanged.

## Display

The user's latest choice supersedes the Open Key display from the earlier
musical-handoffs work. Library and deck labels use Camelot A/B again, with
A = minor and B = major. The deck tooltip explains the suffixes. Stored analysis,
pitch calculations, and the key estimator are unchanged by this follow-up.

## Method

- Same 22 recordings as the earlier Traktor comparison. mixless values are the
  final version-24 analysis from that audit, including Aeipathy at 128 BPM.
- Traktor values are the previous freshly analyzed NML `MUSICAL_KEY` fields,
  not imported `INFO KEY` tags. Details remain in the musical-handoffs report.
- djay Pro 5.6.10 (67091): imported the comparison folder through My Files,
  selected all 22 and ran Analyze. Observed progress and completion. Keys were
  blank before analysis; all 22 afterward had results. Collected the completed
  browser/Automix-preview accessibility values with Automix disabled.
- VirtualDJ: bundle plist reports 8.5.9307 / 18.0.9307; its runtime settings
  panel reports v2026-m b9306. Native input worked after the user activated it.
  Its directory tree did not expose the external drive's children, so two
  task-specific comparison folders were created under Music.
- First VirtualDJ run used byte-identical copies, verified by SHA256 before
  import. Explicitly selected folder Batch -> Analyze for BPM etc, observed
  scan progress and completion, then exported the browser CSV. The initial
  pre-analysis values included imported keys and incorrect BPM tag readings;
  they are not reported as detector outputs.
- Second VirtualDJ run removed source metadata with FFmpeg audio stream copy,
  retaining only title and artist plus container/encoder metadata. All 22
  encoded MP4 mdat SHA256 values exactly matched the input recordings. ffprobe
  verified no key/BPM/tempo tag in any of the 22 files. Ran the same explicit
  batch analysis on the new folder and exported another CSV after completion.
  Results below use this second run. No decoder or audio re-encoder was used.
- All 44 Music comparison files retained their pre-analysis full-file hashes
  after the two scans. Original recordings were not edited. The prior audit,
  rather than a new original-file read in this follow-up, established the
  comparison sources' equivalence to the originals.
- Native application storage rejected direct reads; results were collected
  through djay accessibility output and VirtualDJ's supported CSV export.
  No storage permission bypass or settings changes were used to obtain them.
- No playback/listening or expert tonal annotation was performed. Agreements
  identify review candidates; they are not accuracy scores or ground truth.
- Only VirtualDJ was repeated with key/tempo tags removed. Traktor and djay's
  explicit analysis runs used tagged inputs, so their tag independence has not
  been established by the same control. Their agreement is not three
  independent expert annotations.

VirtualDJ method references: [Analyze tracks](https://virtualdj.com/manuals/virtualdj/interface/database/analyze.html),
[CSV export](https://virtualdj.com/manuals/virtualdj/interface/browser/toolbar.html).
Its [useKeyFromTag option](https://virtualdj.com/manuals/virtualdj/appendix/optionslist.html)
can prefer stored tags; the second input set eliminates those key/BPM tags.
The effective setting was not read or changed.

## Key results

| Pair | Exact agreements |
| --- | ---: |
| mixless / Traktor | 11/22 |
| mixless / djay | 15/22 |
| mixless / VirtualDJ | 11/22 |
| Traktor / djay | 17/22 |
| Traktor / VirtualDJ | 10/22 |
| djay / VirtualDJ | 8/22 |

- All four agree on 7 tracks: Spiritbond, Poison, For You, Always, Aeipathy,
  I'm With You, Fire Away, and Indigo Horizon.
- Bloom is the only track where the other three all agree against mixless:
  mixless 8B (C major), all three 7A (D minor). It is the strongest shared
  disagreement to investigate, not a confirmed correction.
- Dreams, Don't trust me, Found You and wHERE the rhythm goes split 2:2:
  VirtualDJ agrees with mixless, while djay agrees with Traktor.
- Offset: mixless 3B, djay 3A, Traktor and VirtualDJ 2A.
- All 22 VirtualDJ keys are identical between tagged and untagged analysis.
- No key estimator was retuned to follow a product or majority vote.

| Track | mixless | Traktor | djay | VirtualDJ |
| --- | --- | --- | --- | --- |
| Spiritbond | 11B | 11B | 11B | 11B |
| Poison | 4A | 4A | 4A | 4A |
| Bloom | 8B | 7A | 7A | 7A |
| For You, Always | 12B | 12B | 12B | 12B |
| rldwide To ge | 10B | 10B | 10B | 10A |
| Aeipathy | 9B | 9B | 9B | 9B |
| Offset | 3B | 2A | 3A | 2A |
| wHERE the rhythm goes | 9B | 6A | 6A | 9B |
| Ice Shards | 10A | 9A | 10A | 10B |
| Still Here | 6B | 5B | 5B | 4B |
| I'm With You | 5B | 5B | 5B | 5B |
| Still Waiting For You | 3B | 3B | 3B | 12A |
| Fire Away (feat. Slayyyter) - Frost Children Remix | 3B | 3B | 3B | 3B |
| Indigo Horizon | 12B | 12B | 12B | 12B |
| Easier to Love You - Vertile Remix | 7B | 7B | 7B | 7A |
| Dreams | 8A | 7A | 7A | 8A |
| Don't trust me | 3B | 2B | 2B | 3B |
| Where Do We Go | 4B | 3B | 4B | 3B |
| POWER-UP! | 11B | 8A | 11B | 7A |
| Back Again | 12A | 12A | 12A | 11B |
| Found You | 10B | 9A | 9A | 10B |
| Eroge | 4A | 4B | 4A | 7B |

## BPM results

The djay CSV-equivalent UI values are rounded to whole BPM. VirtualDJ exports
one decimal. Decimal precision from these displays is not estimator precision.
Only half/normal/double tempo equivalence is used for the comparisons below.
Raw values are retained in the table.

- Traktor and djay: all 22 within 1 BPM of mixless after half/double normalization.
- VirtualDJ's first, tagged-input scan: all 22 within 1 BPM after normalization.
- VirtualDJ's second, untagged-input scan: 19/22 within 1 BPM. Three exceptions:

| Track | mixless | VirtualDJ tagged input | VirtualDJ untagged input | Interpretation |
| --- | ---: | ---: | ---: | --- |
| Ice Shards | 150.011 | 75.0 | 147.5 | Untagged estimate remains about 2.5 BPM lower |
| Still Here | 142.985 | 71.5 | 95.3 | About a 2:3 tempo interpretation; not hidden as half/double agreement |
| Indigo Horizon | 149.989 | 75.0 | 76.9 | Doubling gives 153.8, about 3.8 BPM higher |

Several other raw BPM values changed only between half/double interpretations.
This two-run difference means a comparison using only tagged inputs would
conceal VirtualDJ's three tempo disagreements. It does not establish which
product's tempo is musically correct; beat/onset evidence is needed for that.
The input files also differ in container metadata and were analyzed under new
paths. A follow-up verified identical audio packet timestamps and stream
durations for all 22, but did not establish the effective tag/cache settings.
The BPM differences therefore cannot be attributed to tag removal alone.

| Track | mixless BPM | Traktor BPM | djay displayed BPM | VirtualDJ untagged displayed BPM |
| --- | ---: | ---: | ---: | ---: |
| Spiritbond | 149.994 | 75.000 | 150 | 150.0 |
| Poison | 174.001 | 174.000 | 174 | 87.0 |
| Bloom | 180.009 | 90.000 | 90 | 90.0 |
| For You, Always | 175.001 | 175.000 | 175 | 87.5 |
| rldwide To ge | 193.999 | 97.000 | 97 | 97.0 |
| Aeipathy | 128.000 | 64.000 | 128 | 128.0 |
| Offset | 175.011 | 174.998 | 175 | 87.5 |
| wHERE the rhythm goes | 173.991 | 174.000 | 174 | 173.9 |
| Ice Shards | 150.011 | 75.001 | 150 | 147.5 |
| Still Here | 142.985 | 143.000 | 143 | 95.3 |
| I'm With You | 145.008 | 145.000 | 145 | 145.0 |
| Still Waiting For You | 149.985 | 74.999 | 150 | 150.0 |
| Fire Away (feat. Slayyyter) - Frost Children Remix | 150.002 | 150.000 | 150 | 150.0 |
| Indigo Horizon | 149.989 | 74.999 | 150 | 76.9 |
| Easier to Love You - Vertile Remix | 154.979 | 155.001 | 155 | 77.5 |
| Dreams | 149.996 | 150.000 | 150 | 150.0 |
| Don't trust me | 150.006 | 150.006 | 150 | 150.0 |
| Where Do We Go | 135.003 | 67.500 | 135 | 135.0 |
| POWER-UP! | 132.002 | 132.000 | 132 | 132.0 |
| Back Again | 134.998 | 134.999 | 135 | 135.0 |
| Found You | 150.004 | 75.001 | 150 | 150.0 |
| Eroge | 74.999 | 75.000 | 150 | 150.0 |

## Validation and evidence

Display change, completed in the earlier part of this follow-up:

- `./dev.sh --build`: passed; beta.5 dev debug build at 2026-10-24T08:37:45Z.
- `cargo test --locked -p mixless-desktop badge_tracks_pitch_and_unlocked_tempo -- --test-threads=1`:
  one test passed (197 filtered). An earlier filter matched zero tests and is
  not counted as validation.
- Isolated offline native preview showed Camelot A/B in library rows. Loading
  a track remained pending; deck rendering is covered by the targeted test,
  not claimed as completed native load/playback evidence.

Comparison evidence, all under ignored `target/key-compare-20261024/`:

- `comparison-final.csv` and `comparison-final.json`: complete four-way table.
- `comparison-summary.json`: computed agreement counts and BPM exceptions.
- `virtualdj-after-analysis.csv`: first run with original metadata.
- `virtualdj-untagged-after-analysis.csv`: second run without key/tempo tags.
- `virtualdj-inputs.json` and `virtualdj-untagged-inputs.json`: source paths and
  hashes. The original source-copy manifest remains `source-manifest.json`.
- `djay-after-analysis.ax.txt` and `djay-results.json`: original UI output and
  parsed 22-track results.
- Native screenshots and capture provenance. Partial filenames retain the
  earlier three-product snapshot; the final files supersede that snapshot.

No application source changed during the VirtualDJ continuation. No commit,
push, release, formal app replacement or live mixless-library migration.
Temporary comparison folders remain under Music for reproducibility; a
navigation shortcut inadvertently created during browsing was removed.

Build SHA256: `e5a123ff48930db16dadadf10f44d79220cd146270b12316c3bef507eb2fa4c0`.
