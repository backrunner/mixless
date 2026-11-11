# Local tonality and metadata-free detector comparison

Date: 2026-10-24. Analysis version 26; order version 6. Camelot A/B remains the
display format. This work extends the earlier key audit without changing its
MP4 tag repair or the unrelated pending UI and handoff changes.

## What changed

- Persist the existing high-resolution, tuning-corrected chroma observations
  as `TonalFrame` source-time intervals (4096/22050 second hop, 8192-sample
  spectral window). Local estimates no longer have to use the average of a
  partly covered bar or the lower-resolution transient spectrum.
- Share section/window analysis in `mixless-protocol::tonal`. Contexts end at
  section boundaries and are at most 16 source seconds. This retains the
  detector's existing statistical context scale, not a fixed musical phrase
  duration. Tempo aliases do not change the included audio.
- Keep the top three template candidates and separate exact-key confidence,
  harmonic-family confidence and observed coverage. Correlations/confidences
  are diagnostic scores, not calibrated probabilities or expert annotations.
- Region summaries select a key actually observed in their local contexts,
  with duration/confidence support. They cannot manufacture a third key by
  averaging two differently keyed sections first.
- Overlap scoring aligns context boundaries from both requested source
  intervals. A proposed incoming pitch must be compatible in every context,
  and every context must satisfy the existing 0.55 harmonic-confidence gate.
  Uncertain or unobserved local material cannot authorize an automatic shift.
  The unchanged relative-major/minor treatment avoids confusing mode ambiguity
  with a different diatonic pitch-class family.
- Smooth, handoff, send-tail, catalog and pitch-recovery consumers use the
  shared local interval path. The ordering heuristic uses available region
  evidence; final pair scoring uses candidate overlap intervals. The lookahead
  no longer predicts B→C harmony from two whole-song labels before choosing
  B's departure. Pitch recovery still evaluates a concrete future plan.
- Fix the catalog path ignoring the disabled automatic-pitch option. A
  reliable local match does not override the user's disabled correction.
- Fix whole-track confidence contexts being counted in accepted frames:
  skipping silence could previously stretch a nominal 16-second context
  across unrelated sections. Context boundaries now use elapsed source time.
- New analysis/order versions invalidate stale persisted analyses and BEST
  results. Older serialized payloads deserialize with an empty frame array;
  their measured bar/region evidence remains a coarser compatibility path.
  No live library payloads or manual cues were rewritten during this audit.

The whole-track winner is still useful as a library label. All 22 comparison
recordings retained that winner; this change improves the locality and
uncertainty of decisions rather than relabeling songs to match a competitor.
Full-mixture chroma is conservative evidence for a selective stem overlap,
not a transcription of the particular isolated instruments. Window boundaries
and spectral resolution also limit how precisely a rapid modulation can be
located. No claim of perfect local key recognition is made.

## Traktor and djay controls

Environment: macOS 27.0.1 (26A434); Traktor Pro 4.5.1 21; djay Pro 5.6.10
(67091). Native UI was used for both applications. No playback was started.

1. Created independent per-application copies of the same 22 recordings.
   Key, BPM and tempo tags were absent before import. Traktor cannot contaminate
   djay's inputs through tag writeback.
2. Imported a new Traktor playlist, observed initially blank analysis values,
   explicitly enabled BPM and Key in Special analysis, observed progress and
   completion, then exported NML. Results come from `MUSICAL_KEY`, not `INFO KEY`.
3. The first new djay directory displayed reused analysis results and an
   already-complete analysis dialog. That was not accepted as a fresh run.
   Created a second set with all source metadata removed and unique anonymous
   filenames. The encoded audio was stream-copied, not re-encoded.
4. The anonymous djay files had blank keys before analysis. Observed active
   per-file progress at 0/22, followed by 22/22 completion. Collected all 22
   results from the native accessibility tree, with Automix disabled.
5. Verified all 66 app-input encoded audio payloads against their preparation
   hashes. The 44 djay copies retained their complete file hashes; Traktor
   rewrote metadata in its 22 separate copies. Anonymous copies have identical
   audio packet timestamps, durations, sizes and stream parameters to the
   original recordings (22/22).

| Check | Result |
| --- | ---: |
| Traktor keys unchanged from its tagged run | 22/22 |
| djay keys unchanged from its tagged run | 22/22 |
| Largest Traktor BPM change between runs | 0.000061 BPM |
| djay displayed integer BPM unchanged | 22/22 |
| Mixless / Traktor exact key agreements | 11/22 |
| Mixless / djay exact key agreements | 15/22 |

The previous key disagreements survive removal of the input key/tempo tags.
The control rules out those tags as the explanation for the observed changes
between products on these files. It does not reveal either closed-source
algorithm's internal weighting or establish which output is musically correct.
The initial djay reuse also shows why a different directory alone is not
sufficient evidence of an independent analysis.

## What the disagreements show

Fresh local spectral estimates from the comparison recordings, before stem
structure refinement, include:

| Recording | Example local result | Implication |
| --- | --- | --- |
| Dreams | 0–24.8 s: 7A, confidence 0.78; several later sections: 8A | The alternative global labels are both represented in measured local contexts. |
| Don't trust me | 26.4–90.4 s: 3B, confidence 0.75; 192.8–207.2 s: 2B, confidence 0.56 | A drop and an exit can need different matching decisions. |
| Still Here | 25.2–40.3 s: 5B, confidence 0.65; 154.4–181.3 s: 6B, confidence 0.91 | The two products' whole-song choices do not describe every section. |
| Offset | 3A/3B compete across contexts; whole-song confidence 0.115 | Relative-mode ambiguity is substantial; Traktor's 2A still needs separate musical verification. |
| Bloom | Intro/build favor 7B; later contexts include 7A, 8B and others; whole-song confidence 0.017 | A confident global correction to the competitors' 7A would be unjustified by this estimator alone. |

These are signal-derived hypotheses, not ground-truth modulation annotations.
Section weighting, harmonic-rich timbre and nearby template fits are plausible
mechanisms. Metadata independence is directly demonstrated; the exact internal
cause of each competitor's choice remains unobserved. Majority voting is not
used to train or tune the estimator.

## Validation and evidence boundaries

The frozen pre-change planner and final planner consumed the same fresh
analyses and current playlist order. Chained entry positions, earliest exits
and carried pitch were propagated across each set. A cut is the audit's
short-transition category; fewer cuts alone do not establish better listening
quality. BEST may select a different order, so its rows compare complete sets.

| Playlist / order | Pairs | Fallbacks before → after | Overlaps ≥4 bars before → after | Cuts before → after |
| --- | ---: | ---: | ---: | ---: |
| Downloads / fixed | 1 | 0 → 0 | 1 → 1 | 0 → 0 |
| Downloads / BEST | 1 | 0 → 0 | 1 → 1 | 0 → 0 |
| Electronic / fixed | 21 | 0 → 0 | 15 → 16 | 6 → 5 |
| Electronic / BEST | 21 | 0 → 0 | 16 → 18 | 5 → 3 |
| MUG / fixed | 28 | 22 → 22 | 1 → 1 | 5 → 5 |
| MUG / BEST | 28 | 18 → 18 | 2 → 3 | 8 → 7 |

MUG's high fallback count remains unresolved. The cached-only DnB and Melodic
fixed-order checks retained zero fallbacks; cuts changed from 6 to 5 and from
3 to 2 respectively. Those cached checks do not validate new feature extraction.
Local scoring adds planning work; unnecessary repeated profile extraction was
removed, but concurrent audit timings are not a controlled latency benchmark.

- The current consistent database snapshot has 78 track records. All 53
  currently readable source files were freshly decoded; their complete source
  BLAKE3 hashes match the earlier verified originals. Existing stem evidence
  was applied once to 28 fresh raw analyses, with matching source timing.
- The 25 records in the old DnB/Melodic playlists point to missing files. Their
  additional checks use cached measurements only and do not validate fresh
  extraction. No source files were downloaded or substituted.
- All available recordings are development/regression material. There is no
  independent expert-labeled tonal holdout in this audit.
- Counterexamples cover a short incompatible tail hidden by a confident
  average, conflicting/matched modulations, missing/flat local evidence,
  fractional pitch carry, and local summaries retaining distinct keys.
- Feature transformations cover gain, time translation, source-clock scaling
  and half-time BPM interpretation. A separate PCM test re-extracts spectra
  from two keys separated by silence under gain/time translations. Existing
  detuned major/minor tests cover all 24 roots/modes. These are distinct from
  real-recording semantic accuracy.
- `cargo test --locked --workspace --exclude mixless-desktop`: 613 passed,
  28 ignored. The subsequent catalog-option fix passed the complete mixplan
  suite (179 passed, one ignored), adding one regression test.
- `./dev.sh --build` completes a development bundle. No installation, launch,
  public release or automatic migration of the user's running library occurred.
- A real-stem offline render of Don't trust me → Still Here completed:
  14.96 seconds, peak 0.3335, 2,805 stem blocks, no nonfinite/clipped samples.
  This checks render validity, not subjective transition quality or physical
  audio-device performance. No listening assessment was performed.

Local evidence lives in ignored `target/local-tonal-20261024/`: input manifests,
Traktor NML, djay before/progress/completion/results AX captures, audio identity
and packet checks, 53 fresh analyses, frozen/current planner outputs, tests,
development build logs and the PCM render. No audio or personal databases are
included in the source changes.
