# Layered live mixing and complete vocal releases

## Scope and evidence

This extends the phrase-release work in commit `36810f7`. The reported development pairs are Used to Be → Angelic Sphere, Don’t You Want Me → Crystallized, Crystallized → Dot to Dot (the next track in the captured Best sequence), and Your voice so → Stella. The earlier Alone → Ember behavior remains part of the continuous-set regression.

A fresh SQLite backup captured the live WAL database without changing it. All 77 analyzed source files match the snapshot’s stored content hashes. The six directly reported recordings use analysis version 20 and stem version 2. Crystallized is 48 kHz; the other five are 44.1 kHz. Exact source paths, the database, cached analyses and PCM remain in ignored local artifacts. Four existing playlists contribute 71 adjacent pairs. The captured 29-track Best sequence contributes another 28 transitions, with every completed rate, pitch and source position passed into the next plan. It is a replay of the captured sequence, not a new run of the order optimizer.

These recordings have been inspected during development and earlier work; they are regression samples, not independent holdouts. No new independent listening corpus was available. Synthetic measurements, actual PCM rendering and human listening are separate evidence classes.

## Mechanisms

- `composition/` fits independent outgoing/incoming voice, kit, instrument and bass schedules. It compares nine foreground/kit anchor combinations and independently searches 0.5/1/2/4-bar fade widths. Bass uses a short complementary exchange; voice releases must fit complete measured rests. A selected voice or rhythm layer may continue after other parts have transferred. No production rule uses a title, track ID, playlist or reported timestamp.
- The main overlap can keep the crossfader centered. Channel levels and stem ratios factor the intended per-part gain while compensating the engine’s actual square-root crossfader law. Previous sine/cosine compensation was inconsistent with that law and is corrected in legacy filtered/vocal arrangements too.
- Three analyzed stem groups provide band power. Instrument low-band energy is a bass proxy; it is not a separately measured fourth analysis group. The energy model no longer averages a muted bass into the melody’s mid/high gain. Foreground ownership drives background mid/high attenuation, capped at 12 dB and 1 dB/s; an isolated kit is high-passed only while the other deck supplies the bass. No speculative spectral boost repairs missing energy.
- Voice fades require at least 95% fine-frame coverage, a rest of at least 0.2 seconds and one-eighth bar, and no confident held note. Relative voice power below -40 dB against the other groups is treated as separator leakage. A legacy vocal carry uses the same evidence across its quarter-bar dry release. Short syllables do not prove a continuous sung line can be faded. The final combined gain/EQ/filter/stem descent also checks quiet held notes, not merely one -3 dB threshold crossing.
- During a continuing outgoing peak, the final arrangement must retain its measured weight within about 3 dB over a local bar. One kick transient alone does not establish a sustained deficit. An incoming drop inside the overlap must have all its stems revealed by arrival. If independent fitting fails, older arrangements still pass the final foreground checks; missing evidence cannot authorize the new fit.
- One complete, full-weight Drop satisfies a play-through. A later longer Drop and a generic 90-second listening preference no longer force another run. A weaker early chorus, a partial peak or an unfinished peak still does not satisfy the rule. User OUT cues keep their existing authority.
- Tempo preparation intersects both decks’ absolute ±8% budgets with A’s actual carried offset. It selects the least movement needed and keeps the existing 0.35%/s logarithmic slope bound. A short peak exchange can hold the matched clock even outside a breakdown; the next pair inherits it. When only a terminal boundary cut is safe, its silent incoming deck may be prepared at a bounded speed useful for the following track. This does not retime an audible overlapping fallback.
- Candidate enumeration additionally protects outgoing arrangement starts, alongside incoming peak landings and nearby spans, with at most 36 lengths. Longer cross-key layers require a successful measured fit with no tonal overlap; the old fixed cross-key envelope remains bounded to 32 bars. `ORDER_VERSION=2` marks the changed planning semantics for sorting caches.

All fitting remains on the host. DSP and callback processing are unchanged. Previously reviewed changes and this work are isolated from concurrent library, metadata and UI changes in the primary checkout.

## Regression and counterexamples

Tests cover independent voice carry, centered-fader gain conservation, gain-invariant timing, unknown/incompatible keys with no tonal overlap, continuous fast syllables, quiet held tails, missing frame coverage, one-frame false breaths, a rest near a window’s end, and an arrangement that omits the voice at its first sample. Tempo tests cover carried rates, three tempo scalings, bounded anticipatory entry and an infeasible gap. Policy tests distinguish one complete Drop from a partial entrance; duration tests preserve irregular incoming landings and outgoing arrangement starts. These transformations operate on measured fixtures and do not establish feature-extractor invariance.

The validation artifacts are under ignored `target/layered-mixing-review-20261017/`. Final outputs use the `validated-` prefix; earlier prefixes are development snapshots, not acceptance evidence.

## Final plans and actual PCM

The final captured Best replay gives these source-time windows. Preparation while B is silent is excluded from the overlap columns.

| Pair | Audible outgoing source | Incoming source during overlap | Real overlap |
| --- | --- | --- | --- |
| Alone → Ember | 191.020–229.891 s | 1.735–39.774 s | 23 bars, 38.05 s |
| Used to Be → Angelic Sphere | 66.894–106.318 s | 0.036–38.635 s | 23 bars, 38.04 s |
| Don’t You Want Me → Crystallized | 127.751–153.012 s | 8.953–31.024 s | 16 bars, 23.99 s |
| Crystallized → Dot to Dot | 108.274–130.345 s | 11.290–33.358 s | 16 bars, 23.98 s |
| Your voice so → Stella | 238.708–249.617 s | 1.893–12.560 s | 8 bars, 11.59 s |

Used to Be previously faded during the ongoing line in a 66.894–80.607 s overlap. The final voice control remains at its foreground level through the quiet held tail ending near 105.314 s; the sampled envelope first falls below 0.99 at 105.354 s and below 0.01 at 105.783 s. Angelic Sphere runs at +2 semitones during this handoff. The departure stays at 106.318 s rather than waiting for the file end.

Don’t You Want Me now leaves at automatic Cue 2, after one complete Drop. With the old captured incoming offset, the pair adds eight silent preparation bars before sixteen real overlap bars. In the final continuous replay, the preceding terminal handoff prepares its approximately 160.1 BPM clock and the sixteen-bar overlap begins directly. Still → Don’t You Want Me remains a conservative terminal cut because the earlier candidate faded a sung line; this is an explicit remaining limitation, not a claimed live overlap.

Crystallized now enters Dot to Dot at 11.290 s rather than the weaker 0.256 s intro. The continuous plan includes eight silent preparation bars before the sixteen audible bars. Your voice so keeps its late high-energy material intact and uses an eight-bar release after it; it does not retain the old nineteen-bar overlap across the conflicting section. Its eleven earlier preparation bars are not counted as mixing.

The final renderer uses the actual source-aligned four-stem cache and the offline 48 kHz engine. All four standalone cases complete with finite, in-range samples and expected final rate/pitch. Peaks are 0.5046 (vocal), 0.5308 (tempo), 0.3535 (spectrum), and 0.4547 (energy). The energy render includes a later solo pitch restoration, so its full 116.96 s output is longer than the overlap. Comparable baseline renders use the corresponding plans extracted from `before-best-plans.json`, preserving the same carried entry states; the earlier `pcm-before-*` files instead used native standalone inputs and are not the comparison baseline.

The continuous 29-track render lasts **3347.45 s**, peak **0.53048**. All 28 handoffs complete; actual final source positions agree within 0.03 s, pitch within 0.02 semitone and rate within 0.002. Four original playlists also pass source bounds, actual carried offsets, and non-overlapping entrance/exit staging across 71 adjacent pairs.

The captured Best sequence goes from 11 to 6 zero-bar cuts and from 17 to 20 overlaps of at least four reported bars; those summary lengths can include silent preparation. Across the four other playlists, overlaps of at least four reported bars decrease from 51 to 41 and zero-bar cuts increase from 14 to 20. Thirteen old long overlaps become shorter: ten contain vocal attenuation during active or held voice under the final guard. Two others switch to higher-scored cuts without triggering that vocal check; the remaining pair has a changed preceding completed state. Three other pairs gain long overlaps. These are candidate-policy changes, not proof of an overall improvement in listening quality. Unsafe voice fades are not retained merely to improve overlap counts; conservative cuts remain where no fitted arrangement passes.

Eight compact before/after previews are saved under the local `previews/` folder. No human listening, physical output, or native UI claim is made.

## Validation

Final checks use the isolated `target/alone-ember-build` Cargo directory to avoid artifacts from concurrent primary-checkout work:

- `cargo test --locked --workspace --exclude mixless-desktop`: **557 passed**, 0 failed, 27 ignored.
- `cargo test --locked -p mixless-desktop`: **185 passed**, 0 failed, 7 ignored.
- `./dev.sh --build`: passed on macOS **27.0.1 (26A434)**. Development bundle 0.1.0-beta.4, revision `v0.1.0-beta.4-12-g36810f7-dirty`, build timestamp `2026-10-17T13:28:26Z`.
- Built executable SHA256: `4a64817acbf95d43546ed363476577fe0f91353e24d860a31c00d6bc56752989`.
- Rustfmt on changed files and `git diff --check`: passed.

The development bundle was built but not launched or installed over the running app. DSP/callback code is unchanged, so callback-budget tests were not rerun. The full source, plan, baseline/updated PCM, preview and counterexample evidence remains local; no recordings, personal database or generated audio are committed.
