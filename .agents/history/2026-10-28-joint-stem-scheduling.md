# Joint stem scheduling and following-pair constraints

Retains the v19 vocal-association and v20 arrangement changes. Planner/order cache version is 21; analysis version 26 and stem version 2 are unchanged. No genre, title, path or recording-ID branches were introduced.

## Mechanism

The old part search could spend its few vocal candidates in the same breath, couple the kit to the melodic release, and require every incoming lane to finish before a peak even when that lane was silent. It also limited automatic source entries to the first half. A locally attractive entry or solo key restoration could then consume the next track's useful departure.

Additional arrangements search distinct measured rests and separate outgoing/incoming melodic phrases. Drums and bass have independent exchange anchors. Shorter structural melodic fades still pass held-note and phrase protection. Incoming peak acceptance measures actual routed total and foreground weight over its first source bar. Whole words, sounding held notes, incompatible foreground overlap and sustained missing foreground remain guarded. A continuing outgoing drum layer alone no longer claims ownership of the previous vocal/melodic arrangement.

Late automatic entries append measured breakdown/recovery opportunities with a complete peak ahead; ordinary intro candidates and explicit cues remain available. A bounded shortlist retains distinct established incoming sections. Following-pair evaluation uses the actual established entry, end time, carried rate/pitch and cues, sharing the existing audible transition cost. Solo pitch restoration reserves the next departure instead of displacing it toward the tail.

## Validation and limitations

Reproduction artifacts are in `target/automix-dnb-joint-20261028/final/`. `before/` beside that directory preserves the retained dirty v20 source. `planner-baseline` is v20 and `planner-candidate` is the frozen v21 implementation. Source hashes distinguish the frozen code from the later test-only coordinate transformation fixture. The audit runner evaluates fixed order, frozen v20 BEST order and freshly computed BEST over all 76 current recordings in four playlists, including cyclic carried state. These are regression recordings; there is no new independent holdout or listening acceptance.

New tests cover distinct rest selection, a whole-line foreground exchange with continuing drums, active-word rejection, incoming peak weight with gain/time/speed transformations, late breakdown entry without losing the intro, and next-departure reservation. Coordinate transformations do not test audio feature re-extraction. The complete workspace retry passed 709 tests with 28 ignored; the first run hit the previously observed native HID discovery timeout. The retry temporarily paused only this task's audit workers. `./dev.sh --build` refreshed the dev bundle without launching playback.

Ten offline engine renders include the reported DnB changes, mixed-tempo cases and non-DnB counterexamples. Supplied-plan identity, real cached stem attachment, completion, sample format, finite samples and clipping are checked separately from the musical observer. Read `report.md`, `comparison.json`, `renders.json` and the logs for final results; successful rendering is not listening acceptance.

The fixed DnB cycle moves Flicker → Lost & Found from the terminal region to the first recovery and gives Punching Holes → Feel This Good and In The Echo → Power actual overlap. Orbit → Underwater, Underwater → Let me bloom, and Feel This Good → In The Echo still fall back to terminal short handoffs. Take Control → We Are The Energy remains sparse. The broader search also increases planning work and exposes foreground/instrument review flags elsewhere; these remain visible in the whole-corpus comparison rather than being treated as solved from nominal transition lengths.

## Final comparison

All 12 fixed/frozen/fresh comparisons completed. Full cold-start cycle reruns equal the resumed-prefix cycles for all four playlists (80 serialized plans including four context edges). The fixed DnB observer reports tail-with-limited-overlap flags 6 → 4 and heard overlap 40.75 → 66.00 seconds. Fresh DnB BEST reports tail flags 1 → 3, overlap 45.15 → 81.85 seconds, and vocal flags 3 → 1. Its flagged neighbours are Feel This Good → In The Echo, In The Echo → We Are The Energy, and Underwater → Divided Sky. These latter two have some overlap; they are not three hard cuts. The revised ordering is not accepted as universally better.

Other sets retain or add foreground/instrument review events. Electronic and MUG fresh BEST each have two vocal review flags, and observer-unsupported loop/FX cases remain unknown. Expanded search materially increases preparation work; audit timings include concurrent workloads, duplicate equivalence runs and a test pause, so they are not a controlled app latency measurement. The dev changes and ten audio clips are retained for listening review, including counterexamples.
