# AutoMix vocal and implementation review

Reviewed the existing AutoMix changes against the dirty working tree based on
`96516289de017b422ae841a90f7ad9885340e2b8`. The review-start files and hashes are
preserved locally in `target/review-vocal-20261111/`. Concurrent planner batching
changes were retained; this review does not claim ownership of those changes.

## Confirmed issues fixed

1. **Outgoing authority bypassed incoming safety.**
   `foreground::preserved` returned immediately for a manual outgoing cue or
   outgoing loop. That also skipped the incoming singer and instrument envelope
   checks, allowing a fade to reveal an already-started word. The exception now
   belongs only to the outgoing source. Manual IN still authorizes its own cut
   boundary, and manual OUT still authorizes its own release. A source cue does
   not authorize a later envelope to hide the beginning of an incoming word.
2. **Missing stems disabled the incoming vocal fade check.**
   The incoming branch used `is_some_and`, while continuation without stems is
   unknown/released. Consequently even strong coarse vocal evidence could be
   ignored. The branch now uses the same moments/bar fallback as the existing
   outgoing check. A missing detailed frame fails conservatively. No new vocal
   activity threshold was introduced.
3. **Solo tempo restoration consumed a later manual OUT.**
   Recovery reserved the recording's final eight bars but did not receive B's
   exit cues. It could continue past B's explicit departure while extending the
   carried source clock. Lookahead now passes those cues into tempo recovery;
   the existing eight-bar reserve also applies before a future manual OUT.
   Automatic suggestions and Hot cues do not impose this restriction.
4. **Render verification used KEY instead of sounding pitch.**
   `render-audit` and `render-set` compared the raw pitch control with a plan's
   sounding offset. An unlocked deck's rate also changes pitch. Both checks and
   their printed pitch values now use `sounding_pitch_semitones()`.
5. **Render tools missed verified legacy cache identities.**
   A fresh audio-v2 key and full-file fallback could not find stems that desktop
   reuses through its persisted audio-v1 identity. This was observed on the real
   render below. Both tools accept `MIXLESS_AUDIT_LIBRARY`, pointing to a SQLite
   backup, and use `Library::stem_hash` to resolve the same verified file identity
   as desktop. Source validation is retained; matching duration alone never
   authorizes an unrelated cache.

The vocal pattern cache scope was also made non-Send/non-Sync, matching its
thread-local restoration contract. No actual cross-thread scope move was found;
this is type-level hardening rather than a reproduced runtime failure.

`ORDER_VERSION` changed from 25 to 26 because the safety and cue fixes can change
plans and persisted Best order. Analysis payload semantics did not change.

## Review coverage and counterexamples

Inspected vocal activity, supported held notes, inter-word continuation, local
chop evidence and its scoped caches; foreground envelopes and incoming melody;
composition fitting, phrase/voice occupancy and ordered parallel reduction;
tempo/varispeed and pitch recovery; cue ownership, lookahead and carried state;
stem readiness, engine plan compilation/publication and gain routing; desktop
worker progress and order-cache invalidation; stem decoding/resource admission;
and the offline audit consumers.

The new regressions cover:

- Rejecting a half-word reveal with an outgoing manual cue and with an outgoing
  loop. The full LoopOut test now rejects a continuously sung receiving source
  and still produces a LoopOut with a quiet receiving vocal stem.
- Rejecting a sung fade without stems while retaining an instrumental fade.
- Retaining explicit outgoing release and incoming cut authority separately.
- Ending tempo recovery before an imminent manual OUT, while still restoring
  normally for an automatic OUT, Hot cue, or sufficiently late manual OUT.

The corrected vocal counterexamples were run against the review-start
`foreground.rs`: two failed as expected and the manual-outgoing positive control
passed. The tempo OUT counterexample also failed before its fix. Existing tests
retain gain/grid/source-position coverage for vocal continuity, held notes,
repeated chops versus sung lines, harmonic exclusions, carried pitch/rate, stem
availability, skip/manual takeover, and deterministic worker result order.

## Validation

- Final workspace tests: **750 passed, 28 ignored**, using
  `cargo test --locked --workspace --exclude mixless-desktop`.
- Desktop tests: **238 passed, 7 ignored**.
- Release callback budgets: **5 passed**, run serially after the planning and
  render jobs had ended. Command:
  `cargo test --locked --release -p mixless-engine callback_budget -- --ignored --nocapture --test-threads=1`.
  Cue p99 was 1.203 ms at 128 frames and 1.514 ms at 512 frames. The dual-deck
  stems workload had p99 0.989 ms and max 3.417 ms against a 2.667 ms block;
  the test's p99 criterion passed, which is not a guarantee of zero overruns.
- `./dev.sh --build` passed. The generated bundle reports
  `0.1.0-beta.5`, `dev`, revision `v0.1.0-beta.5-1-g9651628-dirty`.
  It was not launched or installed elsewhere.
- Both release render examples built successfully. Changed Rust files were
  formatted; `git diff --check` passed.
- Environment: macOS 27.2 (26B123). Rendering used the offline engine at 48 kHz
  with 256-frame blocks; no physical audio device was opened.
- Fresh current-library SQLite backup, rather than a previous audit snapshot.
  All 76 playlist entries across four playlists have analysis version 26 and
  stem evidence version 2. The other 25 version-23 records in the library were
  not substituted into the playlist audit. Payload and track-record content
  hashes match for all 103 stored analyses. The render additionally verifies
  its two actual source files through the library identity path.
- Fixed-order replay carries entry time, earliest departure, rate and sounding
  pitch. Each input appends its first two tracks, covering both the cyclic
  closure and the first departure after returning to the start.
- One real stem render, track IDs 100→95: **143.28 s**, 48 kHz stereo float32,
  peak **0.4763**, **26,865** blocks with stem attenuation, all samples finite
  and within the output range, completed handoff, final rate **1.0** and pitch
  **+1 semitone** matching the supplied plan. The initial render without the
  backup identity lookup was rejected for missing required stem playback; the
  successful retry used the verified cache lookup. It did not silently fall
  back to whole-track playback.
- The originally reported adjacent pair, 40→42, also rendered from the current
  fixed-cycle carried state: **21.53 s**, PhraseBlend, peak **0.3719**, **4,038**
  blocks with stem attenuation, finite/in-range samples and a completed handoff
  at rate **1.0**, pitch **+1 semitone**. This does not reconstruct the exact
  original live-session state or prove the original freeze's cause.

All four fixed cycles completed. The final 80 plans have finite values,
increasing source/clock curves, in-bounds endpoints, correct adjacent track IDs,
and consistent carried rate/pitch and staging time. These are plan checks, not
80 physical or rendered handoffs.

| Playlist | Planned transitions | At least four bars | Near-end short handoffs | Emergency fallback count |
| --- | ---: | ---: | ---: | ---: |
| Melodic | 12 | 9 | 0 | 0 |
| Electronic | 23 | 17 | 3 | 2 |
| MUG | 30 | 16 | 5 | 2 |
| DnB | 15 | 9 | 5 | 3 |

Emergency fallback uses the audit example's definition (a failed ordinary plan
or its one-bar fallback DryCut), not every use of `summary.used_fallback`.
There are still 13 near-end short handoffs and seven emergency fallbacks. No
before/after whole-corpus comparison was run, so these counts are not claimed
as improvements or regressions caused by this review.

Corpus and bounded Best replay status is recorded in
`target/review-vocal-20261111/audit-results.json`. The smallest full Best search
(11 tracks) exceeded its 180-second limit without publishing an order. The
audit process was stopped at that limit. Fresh Best and its cyclic playback
were therefore **not validated** in this run; ordering latency remains an open
issue. Other compilation/verification work overlapped parts of the corpus run,
so its durations are not controlled performance benchmarks.

Local evidence includes `foreground-before.log`, `workspace-tests-final.log`,
`callback-budget.log`, `analysis-manifest.json`, `plan-invariants.json`, the
fixed-cycle plan/progress files, `render.log`, `render-verified-cache/`,
`reported-pair-render/`, and the scoped `review.patch`. Personal audio,
database copies and generated artifacts remain under ignored `target/`.

## Evidence limits

No thresholds were tuned against these recordings. The four existing sets are
regression material, not independent holdouts. Plan completion does not prove
that every vocal boundary is correctly inferred or that a mix sounds natural.
The coarse no-stems fallback intentionally remains conservative and cannot
locate syllables as precisely as complete separated evidence.

The slow varispeed solo glide is an existing intentional pitch movement; its
numerical slope checks are not listening acceptance for singing or sustained
instruments. This review did not relax the active/held-vocal or harmonic gates
to increase plan availability.

No listening session, native interaction or physical audio-device test was
performed. The original reported Loading freeze was not reproduced from the
user's exact session; the separate lifecycle regressions verify delayed
planning, disconnected workers and recovery after outgoing EOF. Successful
tests and this review do not establish absence of all possible defects.
