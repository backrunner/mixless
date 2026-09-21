//! One candidate: source constraints, musical evidence, ranking, then envelopes.
use super::*;

pub(super) fn pair(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    out: f32,
    input: f32,
    mode: TransitionMode,
    length: u16,
    minimum_score: f32,
) -> Option<MixPlan> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let ga = Grid(a);
    let gb = Grid(b);
    let outgoing_bpm = ga.bpm((out - 0.01).max(0.)) * ctx.offset_a.rate;
    let incoming_bpm = gb.bpm(input) * ctx.offset_b.rate;
    let ratio = outgoing_bpm / incoming_bpm;
    let factor = if (ratio / 2. - 1.).abs() < 0.08 {
        0.5
    } else if (ratio / 0.5 - 1.).abs() < 0.08 {
        2.
    } else {
        1.
    };
    let map = if factor == 1. {
        BarMap::OneToOne
    } else {
        BarMap::TwoToOne {
            outgoing_is_double: factor == 0.5,
        }
    };
    let blend = mode == TransitionMode::BeatBlend;
    let loop_roll = mode == TransitionMode::LoopRoll;
    let detailed = !a.moments.is_empty() && !b.moments.is_empty();
    if options.strategy == Some(S::LoopConstruct) && !loop_roll {
        return None;
    }
    if options.strategy == Some(S::ScratchCut) && (blend || loop_roll) {
        return None;
    }
    let hold = options.strategy == Some(S::EnergyHold) && blend;
    if options.strategy == Some(S::DropCut) && (blend || loop_roll) {
        return None;
    }
    if options.strategy == Some(S::Spinback) && (blend || loop_roll) {
        return None;
    }
    if options.strategy == Some(S::LoopOut) && !blend {
        return None;
    }
    let correction = outgoing_bpm * factor / incoming_bpm;
    if blend && ((correction - 1.).abs() > 0.08 || ga.meter() != gb.meter()) {
        return None;
    }
    if loop_roll && ((correction - 1.).abs() > 0.12 || ga.meter() != gb.meter()) {
        return None;
    }
    let boundary_only = length == 0 && mode == TransitionMode::PhraseBridge;
    let mut n = length.max(1);
    if let Some((min, _)) = user_range(ctx.cues_out, a.sample_rate) {
        n = n.max(((out - ga.floor_bar(min)) / ga.meter()).ceil().max(1.) as u16);
    }
    if blend || loop_roll {
        if let Some((_, max)) = user_range(ctx.cues_in, b.sample_rate) {
            n = n.max(
                ((gb.ceil_bar(max) - input) / (ga.meter() * factor))
                    .ceil()
                    .max(1.) as u16,
            );
        }
    }
    let available = ((out - ga.ceil_bar(options.earliest_outgoing_sec.max(0.))) / ga.meter()
        + 0.00001)
        .floor()
        .max(0.) as u16;
    if !blend && !loop_roll && !boundary_only {
        n = n.min(available);
    }
    if n == 0 || (n > available && !boundary_only) || n > 128 {
        return None;
    }
    let n = n as f32;
    let start_beat = out - n * ga.meter();
    let start = ga.sec(start_beat);
    let end = ga.sec(out);
    // Deterministic rotation between equally valid techniques — the pair, the
    // exit and the incoming entry decide, so the same inputs always produce
    // the same strategy.
    let variety = {
        let mut h = (a.track_id.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h = (h ^ b.track_id.0 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h = (h ^ ((end * 10.) as u32) as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
        h = (h ^ ((input * 10.) as u32) as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
        (h ^ (h >> 29)) as u32
    };
    let mut bin = gb.sec(input);
    if mode == TransitionMode::PhraseBridge && input <= 0.001 {
        // A manually marked pickup can precede the first measured beat.
        // Start this unsynchronized bridge at the cue itself; inventing a beat
        // at zero or forcing the pickup into a beat blend would be misleading.
        if let Some((cue, _)) = user_range(ctx.cues_in, b.sample_rate) {
            bin = bin.min(cue);
        } else if let Some(cue) = ctx
            .cues_in
            .iter()
            .filter(|c| {
                c.user_set
                    && c.kind == mixless_protocol::CueKind::Hot
                    && crate::cue_policy::inferred_cue_kind(b, c.frame)
                        == mixless_protocol::CueKind::In
            })
            .map(|c| c.frame as f32 / b.sample_rate as f32)
            .filter(|sec| gb.floor_bar(*sec) <= 0.001)
            .min_by(f32::total_cmp)
        {
            bin = bin.min(cue);
        }
    }
    let (key, shift, key_reliable) = if blend {
        crate::score::overlap_key_match(
            a,
            b,
            (start, end),
            (bin, gb.sec(input + n * ga.meter() * factor)),
            ctx.offset_a.pitch_semitones,
            ctx.offset_b.pitch_semitones,
        )
    } else {
        None
    }
    .unwrap_or_else(|| {
        window_key_match(
            a,
            b,
            end,
            bin,
            ctx.offset_a.pitch_semitones,
            ctx.offset_b.pitch_semitones,
        )
    });
    let key_shift = if blend && options.harmonic_key_shift && key_reliable && key >= 0.25 {
        shift
    } else {
        0.
    };
    let harmonic =
        key_reliable && ((shift == 0. && key >= 0.85) || (key_shift != 0. && key >= 0.25));
    let recovery = crate::recovery::contains(a, start, end);
    if ((!detailed || (!blend && !loop_roll)) && !crate::vocals::safe_exit(a, end))
        || !crate::vocals::safe_entry(b, bin)
    {
        return None;
    }
    let out_phrase = boundary_quality(a, end)
        .max(crate::recovery::quality(a, end))
        .max(crate::cue_policy::manual_quality(
            a,
            ctx.cues_out,
            end,
            true,
        ));
    let in_phrase = boundary_quality(b, bin).max(crate::cue_policy::manual_quality(
        b,
        ctx.cues_in,
        bin,
        false,
    ));
    let start_phrase = boundary_quality(a, start).max(crate::recovery::quality(a, start));
    let explicit_out = user_range(ctx.cues_out, a.sample_rate).is_some();
    let explicit_in = user_range(ctx.cues_in, b.sample_rate).is_some();
    // A hard cut or FX bridge may only land where the outgoing voice has
    // released — singing up to the boundary is fine, sounding past it is not.
    // A user's own OUT cue stays authoritative.
    if !blend && !loop_roll && !explicit_out && !crate::continuity::voice_released(a, end) {
        return None;
    }
    if !explicit_in && !crate::drops::entry_allowed(b, bin) {
        return None;
    }
    if blend && explicit_out && (out - ga.floor_bar(end)).abs() > 0.01 {
        return None;
    }
    // A metrical downbeat is not automatically the beginning of a phrase.
    // Strong structural evidence takes precedence over the old fixed offsets.
    if (!a.phrase_boundaries.is_empty() && !explicit_out && out_phrase < 0.5)
        || (!b.phrase_boundaries.is_empty() && !explicit_in && in_phrase < 0.5)
        || (blend && !a.phrase_boundaries.is_empty() && !explicit_out && start_phrase < 0.5)
    {
        return None;
    }
    if feature(b, bin + 0.001).is_some_and(|bar| bar.rms <= 0.001)
        || feature(a, end - 0.001).is_some_and(|bar| bar.rms <= 0.001)
    {
        return None;
    }
    if end > a.duration_sec + 0.001
        || (start < options.earliest_outgoing_sec && !boundary_only)
        || bin >= b.duration_sec
    {
        return None;
    }
    if !covers_user_range(start, end, ctx.cues_out, a.sample_rate) {
        return None;
    }
    let timing::Timing {
        mut lanes,
        clock,
        source_a,
        source_b,
        master,
        b_end,
        start_b,
    } = timing::prepare(
        ctx,
        mode,
        n,
        start_beat,
        input,
        bin,
        factor,
        hold,
        key_shift,
        harmonic,
        explicit_in,
    )?;
    // Exiting onto the downbeat that resolves A's build throws the tension
    // away unless B's own drop lands exactly where A leaves (a build-up
    // swap). A short suspension at a peak end still owes its next downbeat.
    let bar_a = 240. / outgoing_bpm;
    let leaving_into_drop = !explicit_out
        && crate::drops::next_peak_start(a, end).is_some_and(|p| p <= end + 8.5 * bar_a)
        && (mixless_protocol::drop_suspension(a, end).is_some()
            || !crate::drops::peaks(a)
                .iter()
                .any(|p| (p.1 - end).abs() < 0.05));
    if leaving_into_drop {
        let lands = if blend || loop_roll { b_end } else { bin };
        if mixless_protocol::drop_suspension(a, end).is_some()
            || !crate::drops::peaks(b)
                .iter()
                .any(|p| (p.0 - lands).abs() < 0.08)
        {
            return None;
        }
    }
    // The incoming peak may resolve at the overlap's end or later; it must
    // never start underneath the blend and play filtered beneath A.
    if (blend || loop_roll)
        && !explicit_in
        && crate::drops::peaks(b)
            .iter()
            .any(|p| p.0 >= bin - 0.05 && p.0 < b_end - 0.05)
    {
        return None;
    }
    if !detailed
        && (!fits_region(a, mixless_protocol::MixRegionKind::Out, end, start, end)
            || !fits_region(b, mixless_protocol::MixRegionKind::In, bin, bin, b_end))
    {
        return None;
    }
    if blend && (!grid_reliable(a, start, end) || !grid_reliable(b, bin, b_end)) {
        return None;
    }
    // LoopOut: the outgoing deck loops its final phrase while stems or a deep
    // EQ strip remove its foreground and the incoming track takes over on the
    // phrase. The loop window is the only outgoing material ever heard, so it
    // is the part that must be vocal-safe unless stems strip it anyway.
    let stem_pair = options.stem_playback && a.stems.is_some() && b.stems.is_some();
    let loop_bars = if n >= 8. { 2. } else { 1. };
    let loop_end = ga.sec(start_beat + loop_bars * ga.meter());
    let loop_vocal_safe = a
        .bars
        .iter()
        .filter(|bar| bar.start_sec < loop_end - 0.01 && bar.end_sec > start + 0.01)
        .all(|bar| bar.vocal_presence < 0.5);
    let loop_out = blend
        && (n == 4. || n == 8.)
        && options.strategy.is_none_or(|s| s == S::LoopOut)
        && out_phrase >= 0.75
        && in_phrase >= 0.6
        && start_phrase >= 0.5
        && feature(a, start + 0.01).is_some_and(|f| f.kick_salience >= 0.5)
        // The midpoint hands the floor to B; stripping a driving outgoing
        // loop must not reveal an empty incoming low end.
        && feature(b, source_b.sample(n / 2.) + 0.01)
            .is_some_and(|f| f.kick_salience >= 0.35)
        && (loop_vocal_safe || stem_pair)
        && (crate::drops::peaks(b)
            .iter()
            .any(|p| (p.0 - b_end).abs() < 0.08)
            || matches!(
                section(b, bin + 0.001),
                mixless_protocol::SectionLabel::Intro
                    | mixless_protocol::SectionLabel::Break
                    | mixless_protocol::SectionLabel::Breakdown
            ))
        && (options.strategy == Some(S::LoopOut) || variety % 3 == 0);
    if loop_out {
        lanes.loop_a = Some(mixless_protocol::LoopOp {
            start_src_frame: (start * a.sample_rate as f32).round().max(0.) as u64,
            length_bars: loop_bars as u16,
            on_bar: 0.,
            off_bar: n,
            length_src_frames: ((loop_end - start) * a.sample_rate as f32).round().max(1.) as u64,
        });
    }
    let layer_peak = blend
        && n >= 8.
        && a.sections.iter().any(|s| {
            matches!(
                s.label,
                mixless_protocol::SectionLabel::Drop | mixless_protocol::SectionLabel::Chorus
            ) && start < s.end_sec - 0.05
                && end > s.end_sec + 0.05
        });
    let quiet_layer = layer_peak
        && (average_rms(b, bin, b_end).unwrap_or(0.)
            < average_rms(a, start, end).unwrap_or(1.) * 0.6
            || feature(b, bin + 0.01).is_some_and(|f| f.kick_salience < 0.35));
    let filtered = blend
        && !loop_out
        && (!harmonic || quiet_layer)
        && n >= 4.
        && options.strategy.is_none()
        && (crate::filtered::eligible(b, bin, b_end)
            || (n >= 8. && crate::filtered::layerable(b, bin, b_end)));
    // A filtered blend that runs for many phrases layers two full tracks; it
    // is only a true sweep when at least one side is bare percussion.
    if filtered && n > 16. && !percussion_only(a, start, end) && !percussion_only(b, bin, b_end) {
        return None;
    }
    let tonal_clash = blend
        && !harmonic
        && !filtered
        && !percussion_only(a, start, end)
        && !percussion_only(b, bin, b_end);
    // With stem playback on both decks the clash is avoidable by construction:
    // B's drums carry the layer alone until the handoff while its tonal
    // material stays silent, so a long beat blend remains available.
    let stem_layer = tonal_clash
        && options.stem_playback
        && a.stems.is_some()
        && b.stems.is_some()
        && ((8. ..=32.).contains(&n) || loop_out);
    if tonal_clash && !stem_layer {
        return None;
    }
    // Do not let a long intro blend consume the start of an incoming vocal hook
    // merely because the outgoing side is instrumental. End it on a phrase.
    let end_phrase = boundary_quality(b, b_end).max(
        if recovery && feature(b, b_end - 0.01).is_some_and(|f| crate::vocals::risk(f) < 0.35) {
            0.65 // leave the remaining instrumental phrase playing on B
        } else {
            0.
        },
    );
    if blend
        && !detailed
        && !b.phrase_boundaries.is_empty()
        && !explicit_in
        && end_phrase < 0.5
        && !(recovery && feature(b, b_end - 0.01).is_some_and(|f| crate::vocals::risk(f) < 0.35))
    {
        return None;
    }
    let mut clash = 0.;
    let mut vocal_a = 0.;
    let mut vocal_b = 0.;
    let mut clash_run = 0.;
    let samples = (n * ga.meter() * 4.) as usize;
    for j in 0..samples {
        let t = (j as f32 + 0.5) / samples as f32;
        let (ta, tb) = if blend || loop_roll {
            let u = n * t;
            let mut ta = source_a.sample(u);
            if let Some(op) = &lanes.loop_a {
                // The looped deck wraps inside its phrase; feature sampling
                // must hear what is actually playing.
                let origin = op.start_src_frame as f32 / a.sample_rate as f32;
                let length = op.length_src_frames as f32 / a.sample_rate as f32;
                if u >= op.on_bar && u < op.off_bar && length > 0. {
                    ta = origin + (ta - origin).rem_euclid(length);
                }
            }
            let mut tb = source_b.sample(u);
            if let Some(op) = &lanes.loop_b {
                let length = op.length_src_frames as f32 / b.sample_rate as f32;
                let repeated = if u < op.off_bar {
                    ((source_b.sample(u) - bin) / length + 0.0001).floor()
                } else {
                    ((source_b.sample(op.off_bar) - bin) / length).round() - 1.
                };
                tb -= repeated * length;
            }
            (ta, tb)
        } else {
            // Only compare the foregrounds at the actual bridge boundary.
            (end - (1. - t) * 60. / outgoing_bpm, bin + t * (b_end - bin))
        };
        let va = feature(a, ta).map_or(1., |f| {
            if harmonic || filtered {
                crate::vocals::risk(f)
            } else {
                f.vocal_presence
            }
        });
        let vb = feature(b, tb).map_or(1., |f| {
            if harmonic || filtered {
                crate::vocals::risk(f)
            } else {
                f.vocal_presence
            }
        });
        vocal_a += va / samples as f32;
        vocal_b += vb / samples as f32;
        if va > 0.5 && vb > 0.5 {
            if !stem_layer
                && (!detailed || n < 4.)
                && matches!(
                    (section(a, ta), section(b, tb)),
                    (
                        mixless_protocol::SectionLabel::Verse,
                        mixless_protocol::SectionLabel::Chorus
                    ) | (
                        mixless_protocol::SectionLabel::Chorus,
                        mixless_protocol::SectionLabel::Verse
                    )
                )
            {
                return None;
            }
            clash += 1. / samples as f32;
            clash_run += ga.meter() * n / samples as f32;
            // A long overlap must not dilute a whole colliding vocal line to
            // an acceptable percentage. Allow at most one beat of spillover.
            if !stem_layer && (blend || loop_roll) && (!detailed || n < 4.) && clash_run > 1. {
                return None;
            }
        } else {
            clash_run = 0.;
        }
    }
    // Stem envelopes decide which vocals are actually heard, so the overlap is
    // not a clash for scoring or FX policy; vocal_a/vocal_b stay measured.
    if stem_layer {
        clash = 0.;
    }
    if (blend || loop_roll) && (!detailed || n < 4.) && clash > 0.0625 {
        return None;
    }
    let sa = section(a, end - 0.001);
    let sb = section(b, bin + 0.001);
    let drop_cut = !blend
        && !loop_roll
        && (correction - 1.).abs() <= 0.08
        && grid_reliable(a, start, end)
        && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec))
        && matches!(sa, mixless_protocol::SectionLabel::BuildUp)
        && matches!(sb, mixless_protocol::SectionLabel::Drop)
        && out_phrase >= 0.6
        && in_phrase >= 0.6
        && a.sections.iter().any(|s| {
            s.label == mixless_protocol::SectionLabel::BuildUp
                && (s.end_sec - end).abs() < 0.08
                && mixless_protocol::has_buildup(&a.bars, s.start_sec, s.end_sec)
        })
        && b.sections.iter().any(|s| {
            matches!(s.label, mixless_protocol::SectionLabel::Drop)
                && (s.start_sec - bin).abs() < 0.08
        })
        && feature(b, bin + 0.01).is_some_and(|f| f.kick_salience >= 0.5);
    // Scratch cuts are rare, cueable accents. Require reliable grids,
    // phrase edges, strong kicks and a user hot cue at the incoming hit.
    let scratch_cut = !blend
        && !loop_roll
        && out_phrase >= 0.75
        && in_phrase >= 0.75
        && grid_reliable(a, start, end)
        && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec))
        && feature(a, end - 0.01)
            .is_some_and(|f| f.kick_salience >= 0.55 && f.vocal_presence < 0.35)
        && feature(b, bin + 0.01)
            .is_some_and(|f| f.kick_salience >= 0.55 && f.vocal_presence < 0.65)
        && ctx.cues_in.iter().any(|cue| {
            cue.user_set
                && matches!(cue.kind, mixless_protocol::CueKind::Hot)
                && (cue.frame as f32 / b.sample_rate as f32 - bin).abs() <= 60. / incoming_bpm
        })
        && (options.strategy == Some(S::ScratchCut) || drop_cut || out_phrase >= 0.9);
    // A backspin stop needs a strong outgoing kick on an unsung tail, a
    // reliable grid on both sides, and the incoming drop landing exactly on
    // the bridge boundary — the spin releases onto B's first hit.
    let spinback = !blend
        && !loop_roll
        && n >= 1.
        && out_phrase >= 0.75
        && in_phrase >= 0.6
        && grid_reliable(a, start, end)
        && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec))
        && matches!(
            sa,
            mixless_protocol::SectionLabel::Drop
                | mixless_protocol::SectionLabel::Chorus
                | mixless_protocol::SectionLabel::BuildUp
                | mixless_protocol::SectionLabel::Break
        )
        && crate::drops::peaks(b)
            .iter()
            .any(|p| (p.0 - bin).abs() < 0.08)
        && feature(a, end - 0.01).is_some_and(|f| f.kick_salience >= 0.5 && f.vocal_presence < 0.5)
        && feature(b, bin + 0.01).is_some_and(|f| f.kick_salience >= 0.55)
        && (options.strategy == Some(S::Spinback) || options.strategy.is_none());
    if !explicit_out && crate::arrangement::breaks_build(a, end, b, if blend { b_end } else { bin })
    {
        return None;
    }
    let instant = !blend
        && !loop_roll
        && n == 1.
        && !drop_cut
        && !scratch_cut
        && !spinback
        && out_phrase >= 0.6
        && in_phrase >= 0.6
        && crate::continuity::cut_safe(a, end, false)
        && !matches!(sa, mixless_protocol::SectionLabel::BuildUp);
    if boundary_only && !instant && !drop_cut && !spinback {
        return None;
    }
    if !blend && !loop_roll && n < 4. && !drop_cut && !scratch_cut && !instant && !spinback {
        return None;
    }
    if drop_cut && !crate::continuity::cut_safe(a, end, true) {
        return None;
    }
    let fx_decision = if blend || instant {
        policy::Decision {
            technique: Technique::DryCut,
            confidence: if instant
                && (!grid_reliable(a, start, end) || !grid_reliable(b, bin, b_end))
            {
                0.6
            } else {
                1.0
            },
        }
    } else {
        policy::choose(FxEvidence {
            loop_roll,
            scratch: scratch_cut,
            drop: drop_cut,
            spinback,
            variety,
            grid_reliable: grid_reliable(a, start, end)
                && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec)),
            phrase_reliable: out_phrase >= 0.6 && in_phrase >= 0.6,
            harmonic_compatible: harmonic,
            vocal_overlap: clash,
            outgoing_kick: feature(a, end - 0.01).map_or(0.0, |f| f.kick_salience),
            incoming_kick: feature(b, bin + 0.01).map_or(0.0, |f| f.kick_salience),
            outgoing_vocal: vocal_a,
            incoming_vocal: vocal_b,
            outgoing_section: sa,
        })
    };
    if !explicit_out
        && !crate::drops::allows(
            a,
            start,
            end,
            options.outgoing_entry_sec,
            filtered
                || instant
                || matches!(
                    fx_decision.technique,
                    Technique::DropCut | Technique::ScratchCut | Technique::Spinback
                ),
        )
    {
        return None;
    }
    if (loop_roll && fx_decision.technique != Technique::LoopRoll)
        || (options.strategy == Some(S::DropCut) && fx_decision.technique != Technique::DropCut)
        || (options.strategy == Some(S::ScratchCut)
            && fx_decision.technique != Technique::ScratchCut)
        || (options.strategy == Some(S::Spinback) && fx_decision.technique != Technique::Spinback)
        || (options.strategy == Some(S::LoopOut) && !loop_out)
    {
        return None;
    }
    let handoff = if filtered {
        n - 1.
    } else if blend {
        if loop_out {
            n / 2.
        } else {
            bass_handoff(a, b, &source_a, &source_b, n)?
        }
    } else if let Some(op) = &lanes.loop_b {
        op.off_bar
    } else {
        start_b
    };
    let rms_a = if blend {
        average_rms(
            a,
            source_a.sample((handoff - 2.).max(0.)),
            source_a.sample((handoff + 2.).min(n)),
        )
    } else {
        average_rms(
            a,
            (end - 4. * ga.meter() * 60. / outgoing_bpm).max(start),
            end,
        )
    };
    let rms_b = if blend {
        average_rms(
            b,
            source_b.sample((handoff - 2.).max(0.)),
            source_b.sample((handoff + 2.).min(n)),
        )
    } else {
        average_rms(
            b,
            bin,
            (bin + 4. * gb.meter() * 60. / incoming_bpm).min(b.duration_sec),
        )
    };
    let score = ranking::Evidence {
        start,
        end,
        bin,
        n,
        mode,
        explicit_out,
        phrases: [out_phrase, in_phrase, start_phrase, end_phrase],
        voices: [vocal_a, vocal_b],
        rms: [rms_a, rms_b],
        key,
        key_shift,
        clash,
        drop_cut,
        scratch_cut,
        spinback: fx_decision.technique == Technique::Spinback,
        loop_out,
        stem_pair,
        fx_decision,
    }
    .score(ctx, options)?
        - if filtered { 0.10 } else { 0. }
        // Stem-layered tonal material never overlaps, so there is no measured
        // progression tension; a flat penalty keeps true harmonic blends ahead.
        - if stem_layer {
            0.06
        } else if blend && !filtered {
            crate::progression::tension(
                a,
                b,
                &source_a,
                &source_b,
                n,
                ctx.offset_a.pitch_semitones,
                ctx.offset_b.pitch_semitones + key_shift,
            ) * 0.16
        } else {
            0.
        };
    if score <= minimum_score {
        return None;
    }
    // Every decoded source has its own gated loudness trim. Pair-relative RMS
    // boosts would undo it, amplify quiet intros and accumulate across a set.
    let trim_b = 0.;
    let rhythmic_handoff = loop_roll
        || blend
            && (feature(a, source_a.sample(handoff) - 0.01)
                .is_some_and(|f| f.kick_salience >= 0.5)
                || feature(b, source_b.sample(handoff) + 0.01)
                    .is_some_and(|f| f.kick_salience >= 0.5));
    lanes::shape(
        &mut lanes,
        blend || loop_roll,
        loop_roll,
        n,
        handoff,
        start_b,
        rhythmic_handoff,
        ga.meter(),
        trim_b,
        vocal_a,
        vocal_b,
        outgoing_bpm,
        if loop_out {
            Technique::LoopOut
        } else {
            fx_decision.technique
        },
        stem_pair,
    );
    let stages = crate::choreography::arrange(
        &mut lanes,
        mode,
        n,
        handoff,
        ga.meter(),
        trim_b,
        harmonic,
        rhythmic_handoff,
        vocal_a,
        vocal_b,
        sa,
        fx_decision.technique,
    );
    if scratch_cut {
        let on_bar = (n - 0.5).max(0.0);
        let peak_bar = (n - 0.25).max(on_bar + 0.0625);
        let start_src_frame = (source_a.sample(on_bar) * a.sample_rate as f32)
            .round()
            .max(0.0) as u64;
        let delta = (0.5 * a.sample_rate as f32 * 60. / outgoing_bpm).round() as i64;
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame,
            peak_delta_frames: -delta,
            on_bar,
            peak_bar,
            off_bar: n,
            accelerate: false,
        });
    }
    if fx_decision.technique == Technique::Spinback {
        // The spin pulls the last half bar of A backwards, releasing exactly
        // onto B's downbeat. Cap the pull below the engine's 2 s bound.
        let on_bar = (n - 0.5).max(0.);
        let start_src_frame = (source_a.sample(on_bar) * a.sample_rate as f32)
            .round()
            .max(0.) as u64;
        let pull = ((1.5 * ga.meter() * 60. / outgoing_bpm).min(1.9) * a.sample_rate as f32).round()
            as i64;
        if start_src_frame < pull as u64 {
            return None;
        }
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame,
            peak_delta_frames: -pull,
            on_bar,
            peak_bar: n - 0.02,
            off_bar: n,
            accelerate: true,
        });
    }
    // A never jumps on the first sample; B's pitch is set while inaudible and
    // held thereafter. A key change is made by handing over musical material.
    let ending = mixless_protocol::PerformanceOffset {
        rate: lanes.rate_b.sample(n),
        pitch_semitones: ctx.offset_b.pitch_semitones + key_shift,
    };
    let mut plan = MixPlan {
        stem_mix: None,
        stages,
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: if loop_out {
                S::LoopOut
            } else if stem_layer {
                S::PhraseBlend
            } else if blend {
                if hold {
                    S::EnergyHold
                } else if !harmonic || !rhythmic_handoff {
                    S::PhraseBlend
                } else {
                    S::BassSwap
                }
            } else if loop_roll {
                S::LoopConstruct
            } else {
                match fx_decision.technique {
                    Technique::DryCut => S::DryCut,
                    Technique::ScratchCut => S::ScratchCut,
                    Technique::DropCut => S::DropCut,
                    Technique::Spinback => S::Spinback,
                    Technique::FilterBridge => S::FilterSweep,
                    _ => S::EchoOut,
                }
            },
            score,
            used_fallback: !blend
                && !loop_roll
                && !matches!(
                    fx_decision.technique,
                    Technique::DropCut | Technique::ScratchCut | Technique::Spinback
                )
                && (fx_decision.technique != Technique::DryCut || fx_decision.confidence < 0.8),
            length_bars: n as u16,
        }),
        incoming_offset_end: ending,
        outgoing_offset: ctx.offset_a,
        bar_map: if blend || loop_roll || drop_cut {
            map
        } else {
            BarMap::OneToOne
        },
        t_in_a: start,
        t_out_a: end,
        t_in_b: bin,
        t_end_b: b_end,
        clock,
        incoming_source: source_b,
        outgoing_source: source_a,
        incoming_start_bar: start_b,
        transition_mode: Some(mode),
        master_bpm: master,
        lanes,
        handoff_bar: Some(handoff),
        literal_half_double: false,
        failure_reason: None,
        requires_stems: false,
    };
    if loop_out {
        plan.stages = vec![
            mixless_protocol::MixStage {
                start_bar: 0.,
                end_bar: n / 2.,
                label: "Loop outgoing phrase".into(),
            },
            mixless_protocol::MixStage {
                start_bar: n / 2.,
                end_bar: n / 2. + 0.25,
                label: "Bass exchange".into(),
            },
            mixless_protocol::MixStage {
                start_bar: n / 2. + 0.25,
                end_bar: n,
                label: "Release loop · outgoing out".into(),
            },
        ];
        if stem_pair {
            crate::stem_mix::loop_out(&mut plan);
        }
    }
    if mode == TransitionMode::PhraseBridge
        && !instant
        && !matches!(
            fx_decision.technique,
            Technique::DropCut | Technique::ScratchCut | Technique::Spinback
        )
    {
        crate::bridge::expand(&mut plan, ctx, fx_decision.technique, harmonic)?;
    }
    if filtered {
        crate::filtered::arrange(&mut plan, a, b)?;
    } else if !loop_out && (blend || loop_roll || plan.incoming_start_bar == 0.) {
        crate::vocals::protect(&mut plan, a)?;
    }
    // A scratch op needs its material: compacting would crop the pull bars.
    if (instant || drop_cut) && plan.lanes.scratch_a.is_none() {
        crate::instant::compact(&mut plan);
    }
    if plan.t_in_a < options.earliest_outgoing_sec {
        return None;
    }
    if stem_layer && !loop_out {
        crate::stem_mix::layer(ctx, &mut plan, handoff);
    }
    Some(plan)
}
