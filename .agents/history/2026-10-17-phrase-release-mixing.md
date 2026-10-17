# Phrase-release mixing: Alone → Ember

## Mechanism and scope

Development pair: Alone (77) → Ember (61). Baseline `c63b388` selected a nine-bar FilterSweep from outgoing source 256.932–272.143 s, almost at the 273.856 s end. The generic main-peak rule requires a run at least 80% as long as the longest remaining run. It treated earlier full-weight, complete peaks as previews solely because the last run was longer.

The set planner now also considers a complete major peak followed by an independently phrased measured release. It requires an observed ending, full coverage of the recovery phrase, at least two such phrase widths in the preceding peak, overall retreat of at least 3 dB, bass retreat of at least 6 dB, and at least one-third fewer attacks. These contrast budgets already exist in suspension analysis. A silent tail, bass mute alone, sustained drums, build-up, partial entry, missing evidence, or weak first chorus cannot establish this opportunity. The earlier of this supported peak and the original main peak is owed. Generic automatic cue semantics and analysis version remain unchanged; manual cues and all candidate safety gates remain authoritative.

The bounded duration search retains its previous candidates and additionally protects up to eight incoming peak landings and eight nearby spans (at most 28 total). Uniform subsampling had removed nonstandard lengths such as 14 and 23 bars, and could remove a still-valid short phrase after a later entrance changed the available length. Rejection tracing now includes bass-handoff, filtered-envelope and vocal-protection failures.

No title, recording ID, playlist or measured timestamp appears in production rules. No DSP or callback processing changed.

## Sources and sample roles

A fresh SQLite backup of the live WAL database was used. All 77 analyzed source hashes matched current files. Analysis version 20, stem version 2. The development pair is 44.1 kHz; cached source-aligned stem PCM was used for rendering. Model hashes:

- separator: `d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a`
- notes: `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`
- Alone: `911eb5c0d599c2cdbef3f45d522ab2dd47b87f4845bde57fbebf803a3d748abf`
- Ember: `8597a69d6c4efd7d2097eff277902d96e96893c34dbf234a242b3aab30efefe2`

Alone's measured first drop is 108.206–135.247 s. Its following breakdown has lower RMS, low-band energy and attacks; the preceding chorus's nominal breakdown does not meet the combined retreat test. This supports the earlier opportunity without assuming every Breakdown label is a release. Remaining pair safety and ranking select a later usable exchange, not an unconditional cut at 135 s.

The four existing playlists are regression samples, not independent holdouts; they were inspected in earlier work. No new independent listening corpus was available. Synthetic counterexamples and gain, source-time offset, speed, and half/double-clock transformations exercise the release rule. They do not establish feature-extractor or subjective accuracy.

## Actual plans and audio

New Alone → Ember: 23-bar BassSwap, outgoing source **191.01955–229.891 s**, incoming source **1.734627–39.773655 s** at overlap end. It layers during Alone's recovery, preserves its foreground with separate stem, level and EQ lanes, exchanges bass, and resolves onto Ember's first drop. Ember is matched at −1 semitone during the overlap. The approximately 38.45 s overlap ends 42.25 source seconds earlier than before.

When no next track is specified, the existing solo pitch-recovery policy later restores Ember to zero at source 115.852 s. With Luolimasi known next, it holds −1 through Ember's solo passage and plans the next handoff from that actual state. The complete chain Angelic Sphere → Alone → Ember → Luolimasi verifies source positions, rates and pitches at every handoff.

Initial actual PCM checks (48 kHz offline engine, no physical audio device): baseline peak 0.4334; new pair peak 0.3998; continuous four-track render 520.34 s, peak 0.51015. All completed, finite and within full scale. The pair's post-overlap solo recovery is included in the full render; separate 48 s previews cover four seconds before the overlap through the drop. These are rendering checks, not a claim that the transitions have been listened to or certified smooth.

Across 71 adjacent pairs, all source bounds, carried offsets and entrance/exit non-overlap checks pass. Eighteen plans change. Played-out terminal handoffs fall from 13 to 9; failure fallbacks from 7 to 5; at least-four-bar overlaps rise from 46 to 51. These counts assess candidate availability only. In-progress validation exposed a regression where a shorter candidate was evicted after a different entry; retaining nearby spans preserves the original 44 → 31 long overlap.

Local artifacts are ignored under `target/alone-ember-review-20261017/`: fresh database, exported analyses, before/final plans, rejection traces, PCM, previews, source-hash verification and plan diffs. Source paths, acquired recordings, databases and generated files are not committed.

## Validation

Validation is completed in an isolated build directory because a concurrent task was also using the primary checkout and Cargo output. The first shared-cache workspace run omitted the two newly added protocol tests and is not counted as final validation. The independent worktree preserves the other task's library, metadata and Best-order changes.

Final isolated results:

- `cargo test --locked --workspace --exclude mixless-desktop`: 544 passed, 0 failed, 27 ignored, including all four new regression/transform tests.
- `cargo test --locked -p mixless-desktop`: 185 passed, 0 failed, 7 ignored.
- `./dev.sh --build`: passed on macOS 27.0.1 (26A434). Dev bundle 0.1.0-beta.4, build revision `v0.1.0-beta.4-11-gc63b388-dirty`; executable SHA256 `843adf57151210896ccb1857ddf06f8065db421b314b700057489026bb28e974`.
- The isolated audit executable reproduced the exact pair and four-track plan JSON. The isolated engine then rendered the pair and continuous chain with the same peaks and completed states as above; regression 14 → 6 rendered 19.04 s (peak 0.3821), and 41 → 44 rendered 14.40 s (peak 0.4000), both using actual stem PCM.
- Rustfmt on changed Rust files and `git diff --check`: passed.

The built bundle is in the attached independent worktree's `target/app/Mixless.app`. It was not launched or installed over the running app. No subjective listening, physical output, or new native UI claim is made. Callback code is unchanged, so callback budget benchmarks were not rerun.
