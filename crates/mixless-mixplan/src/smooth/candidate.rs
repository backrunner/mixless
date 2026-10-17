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
    let plain =
        direct(ctx, options, out, input, mode, length, minimum_score, false).or_else(|| {
            super::preparation::candidate(ctx, options, out, input, mode, length, minimum_score)
        });
    // A vocal carry is judged as its own candidate over the same window, so
    // a bed that cannot carry the voice still leaves the ordinary blend.
    let floor = plain
        .as_ref()
        .and_then(|p| p.summary.as_ref())
        .map_or(minimum_score, |s| s.score.max(minimum_score));
    direct(ctx, options, out, input, mode, length, floor, true).or(plain)
}

pub(super) fn direct(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    out: f32,
    input: f32,
    mode: TransitionMode,
    length: u16,
    minimum_score: f32,
    ride: bool,
) -> Option<MixPlan> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let ga = Grid(a);
    let gb = Grid(b);
    crate::trace::candidate(ga.sec(out), gb.sec(input), length, mode as u8);
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
        return miss!();
    }
    if options.strategy == Some(S::ScratchCut) && (blend || loop_roll) {
        return miss!();
    }
    let requested_hold = options.strategy == Some(S::EnergyHold) && blend;
    if options.strategy == Some(S::DropCut) && (blend || loop_roll) {
        return miss!();
    }
    let accent_strategy = matches!(
        options.strategy,
        Some(S::Spinback | S::TapeStop | S::BuildRoll)
    );
    if accent_strategy && (blend || loop_roll) {
        return miss!();
    }
    if options.strategy == Some(S::LoopOut) && !blend {
        return miss!();
    }
    let correction = outgoing_bpm * factor / incoming_bpm;
    if blend && ((correction - 1.).abs() > 0.08 || ga.meter() != gb.meter()) {
        return miss!();
    }
    if loop_roll && ((correction - 1.).abs() > 0.12 || ga.meter() != gb.meter()) {
        return miss!();
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
        return miss!();
    }
    let n = n as f32;
    let start_beat = out - n * ga.meter();
    let start = ga.sec(start_beat);
    let end = ga.sec(out);
    // A chopped vocal is rhythmic material: it may be cut through and
    // entered over, unlike a sung line. Judged on measured note lengths.
    let chop_a = crate::vocals::chops(a, start, end);
    // A short recovery need not fit the entire tempo migration. Keep A's
    // sounding clock and carry B's offset into the next pair instead of
    // rejecting a safe overlap until the outro. 1.875 is the maximum slope
    // of the quintic ease used by timing::prepare.
    let explicit_out = user_range(ctx.cues_out, a.sample_rate).is_some();
    // A vocal carry: the outgoing arrangement leaves but its sung line goes
    // on over the incoming instrumental, and the fader comes down where the
    // voice finishes. Both decks need aligned stems to separate the voice.
    // The exit is the line's own release, so it may sit inside a section.
    let carry = ride
        && blend
        && options.stem_playback
        && options.strategy.is_none()
        && a.stems.is_some()
        && b.stems.is_some()
        && !explicit_out
        && n >= 8.
        && crate::phrasing::vocal_release(a, end)
        && crate::continuity::voice_active(a, (end - 8.).max(start)) > 0.35;
    if ride && !carry {
        return None;
    }
    let hold = blend
        && (requested_hold
            // The voice keeps the pulse it was sung to: A stays at its own
            // tempo and B is matched underneath it.
            || carry
            || options.strategy.is_none()
                && crate::recovery::contains(a, start, end)
                && 1.875 * correction.ln().abs() / ((end - start) / ctx.offset_a.rate).max(0.01)
                    > MAX_LOG_TEMPO_PER_SEC);
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
    let incoming_end = if blend || loop_roll {
        gb.sec(input + n * ga.meter() * factor)
    } else {
        bin + (end - start) / ctx.offset_a.rate * ctx.offset_b.rate
    };
    let (key, shift, key_reliable) = crate::score::overlap_key_match(
        a,
        b,
        (start, end),
        (bin, incoming_end.min(b.duration_sec)),
        ctx.offset_a.pitch_semitones,
        ctx.offset_b.pitch_semitones,
    )
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
    let key_shift = if options.harmonic_key_shift && key_reliable && key > 0. {
        shift
    } else {
        0.
    };
    let harmonic = key_reliable && ((shift == 0. && key >= 0.85) || key_shift != 0.);
    if carry && !harmonic {
        return miss!();
    }
    let recovery = crate::recovery::contains(a, start, end);
    if ((!detailed || (!blend && !loop_roll)) && !crate::vocals::safe_exit(a, end))
        || !crate::vocals::safe_entry(b, bin)
    {
        return miss!();
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
    let explicit_in = user_range(ctx.cues_in, b.sample_rate).is_some();
    // Every technique finishes A at an evidenced phrase edge. Recovery
    // quality may authorize layering, never an internal exit, even with stems.
    if !explicit_out && !carry && !crate::phrasing::exit_boundary(a, end) {
        return miss!();
    }
    // A recovery interval can support an overlap, but its internal downbeats
    // are not automatically phrase endings for a bridge or performance cut.
    if !blend
        && !loop_roll
        && !explicit_out
        && !a.phrase_boundaries.is_empty()
        && boundary_quality(a, end) < 0.5
    {
        return miss!();
    }
    // A hard cut or FX bridge may only land where the outgoing voice has
    // released — singing up to the boundary is fine, sounding past it is not.
    // A user's own OUT cue stays authoritative.
    // A chopped line needs no breath; only a sung one does.
    if !blend
        && !loop_roll
        && !explicit_out
        && !chop_a
        && !crate::continuity::voice_released(a, end)
    {
        return miss!();
    }
    // The incoming bed plays whole under a carried voice, so a carry may
    // start inside a run the incoming track opens on, never a later peak.
    if !explicit_in
        && !crate::drops::entry_allowed(b, bin)
        && !(carry && crate::drops::opening_peak(b, bin))
    {
        return miss!();
    }
    if blend && explicit_out && (out - ga.floor_bar(end)).abs() > 0.01 {
        return miss!();
    }
    // A metrical downbeat is not automatically the beginning of a phrase.
    // Strong structural evidence takes precedence over the old fixed offsets.
    // A carry leaves A where its line ends, but B still starts its phrase.
    if (!carry && !a.phrase_boundaries.is_empty() && !explicit_out && out_phrase < 0.5)
        || (!b.phrase_boundaries.is_empty() && !explicit_in && in_phrase < 0.5)
        || (blend
            && !carry
            && !a.phrase_boundaries.is_empty()
            && !explicit_out
            && start_phrase < 0.5)
    {
        return miss!();
    }
    // The outgoing side must still be sounding: leaving on A's own fade is a
    // gap, not a transition, and the unmixed tail is what the gap sounds
    // like. A quiet incoming intro is a legitimate entry, so only the
    // outgoing bar is held to this floor.
    // Compare against the track's usual level, not its average: a long quiet
    // intro would drag an average down far enough to legalise ending the mix
    // inside the outgoing track's own fade.
    let floor = {
        let mut levels: Vec<f32> = a
            .bars
            .iter()
            .map(|bar| bar.rms)
            .filter(|r| *r > 0.001)
            .collect();
        levels.sort_by(f32::total_cmp);
        levels
            .get((levels.len().saturating_sub(1)) * 4 / 5)
            .copied()
            .map_or(0., |usual| usual * 0.05)
    };
    if feature(b, bin + 0.001).is_some_and(|bar| bar.rms <= 0.001)
        || feature(a, end - 0.001).is_some_and(|bar| bar.rms <= 0.001)
        || (end < a.duration_sec - 0.5
            && feature(a, end - 0.001).is_some_and(|bar| bar.rms < floor))
    {
        return miss!();
    }
    // Exiting inside a measured silence is never a musical exit: the track
    // has already stopped sounding, so anything after that point is a gap.
    if end < a.duration_sec - 0.5
        && feature(a, end - 0.001)
            .is_some_and(|bar| bar.section == mixless_protocol::SectionLabel::Silence)
    {
        return miss!();
    }

    if end > a.duration_sec + 0.001
        || (start < options.earliest_outgoing_sec && !boundary_only)
        || bin >= b.duration_sec
    {
        return miss!();
    }
    if !covers_user_range(start, end, ctx.cues_out, a.sample_rate) {
        return miss!();
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
    // The bed must carry the voice for the whole window, and the incoming
    // voice must be able to come in on a breath where the line has gone.
    let ride = if carry {
        let fit = crate::ride::fit(
            a,
            b,
            &source_a,
            &source_b,
            n,
            harmonic,
            ctx.offset_a.pitch_semitones,
            ctx.offset_b.pitch_semitones + key_shift,
        )
        .or_else(|| miss!())?;
        let shape = crate::ride::shape(
            a,
            b,
            &source_a,
            &source_b,
            n,
            ((outgoing_bpm * factor) / incoming_bpm - 1.).abs() <= 0.08,
        );
        // The incoming voice comes back on a breath, or on a strong phrase
        // line where it swells in rather than cutting in mid-word.
        let back = source_b.sample(shape.voice_end.min(n));
        if !crate::vocals::safe_entry(b, back) && boundary_quality(b, source_b.sample(n)) < 0.6 {
            return miss!();
        }
        if !crate::drops::allows(
            a,
            start,
            source_a.sample(shape.leave),
            options.outgoing_entry_sec,
            false,
        ) {
            return miss!();
        }
        Some((fit, shape))
    } else {
        None
    };
    // A bridge of several bars lets B establish itself underneath A, so the
    // level that must carry on is B's where the bridge ends.
    let takeover = if blend || loop_roll {
        b_end
    } else if n >= 2. {
        gb.sec(input + n * gb.meter()).min(b.duration_sec - 0.1)
    } else {
        bin
    };
    if !explicit_in && !crate::energy::handoff_supported(a, b, end, takeover) {
        return miss!();
    }
    // Exiting onto the downbeat that resolves A's build throws the tension
    // away unless B's own drop lands exactly where A leaves (a build-up
    // swap). A short suspension at a peak end still owes its next downbeat.
    let leaving_into_drop = !explicit_out
        && (mixless_protocol::drop_suspension(a, end).is_some()
            || crate::drops::next_peak_start(a, end).is_some_and(|p| (p - end).abs() < 0.08)
            || a.sections.iter().any(|s| {
                s.label == mixless_protocol::SectionLabel::BuildUp
                    && end > s.start_sec + 0.08
                    && end <= s.end_sec + 0.08
            }));
    if leaving_into_drop {
        let lands = if blend || loop_roll { b_end } else { bin };
        if mixless_protocol::drop_suspension(a, end).is_some()
            || !(crate::drops::peaks(b).iter().any(|p| (p.0 - lands).abs() < 0.08)
                // A single, continuous peak has no surrounding sections for
                // peak_ranges to merge, but can still receive a build-up swap.
                || b.sections.len() == 1 && b.sections.iter().any(|s| {
                    matches!(s.label, mixless_protocol::SectionLabel::Drop | mixless_protocol::SectionLabel::Chorus)
                        && (s.start_sec - lands).abs() < 0.08
                }))
        {
            return miss!();
        }
    }
    // The incoming peak may resolve at the overlap's end or later; it must
    // never start underneath the blend and play filtered beneath A.
    if !carry
        && (blend || loop_roll)
        && !explicit_in
        && crate::drops::peaks(b)
            .iter()
            .any(|p| p.0 >= bin - 0.05 && p.0 < b_end - 0.05)
    {
        return miss!();
    }
    if !detailed
        && (!fits_region(a, mixless_protocol::MixRegionKind::Out, end, start, end)
            || !fits_region(b, mixless_protocol::MixRegionKind::In, bin, bin, b_end))
    {
        return miss!();
    }
    if blend && (!grid_reliable(a, start, end) || !grid_reliable(b, bin, b_end)) {
        return miss!();
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
        && !carry
        && !loop_out
        && (!harmonic || quiet_layer)
        && n >= 4.
        && options.strategy.is_none()
        && (crate::filtered::eligible(b, bin, b_end)
            || (n >= 8. && crate::filtered::layerable(b, bin, b_end)));
    // A filtered blend that runs for many phrases layers two full tracks; it
    // is only a true sweep when at least one side is bare percussion.
    if filtered && n > 16. && !percussion_only(a, start, end) && !percussion_only(b, bin, b_end) {
        return miss!();
    }
    let tonal_clash = blend
        && !carry
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
        return miss!();
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
        return miss!();
    }
    let chop_b = crate::vocals::chops(b, bin, b_end);
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
        let level = |t: &TrackAnalysis, sec: f32| {
            if harmonic || filtered {
                feature(t, sec).map_or(1., crate::vocals::risk)
            } else {
                feature(t, sec).map_or(1., |f| f.vocal_presence)
            }
        };
        // A chop is foreground rhythm, not a lyric: it does not own the
        // window the way a sung line does.
        let va = if chop_a { 0. } else { level(a, ta) };
        let vb = if chop_b { 0. } else { level(b, tb) };
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
                return miss!();
            }
            clash += 1. / samples as f32;
            clash_run += ga.meter() * n / samples as f32;
            // A long overlap must not dilute a whole colliding vocal line to
            // an acceptable percentage. Allow at most one beat of spillover.
            if !stem_layer && (blend || loop_roll) && (!detailed || n < 4.) && clash_run > 1. {
                return miss!();
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
        return miss!();
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
        && (accent_strategy
            || (options.strategy.is_none() && options.live_moves == crate::LiveMoves::Active));
    // A roll repeats the outgoing last bar's opening beat: it must be a
    // driving, vocal-free beat inside a build or a drop.
    let roll = spinback
        && matches!(
            sa,
            mixless_protocol::SectionLabel::BuildUp | mixless_protocol::SectionLabel::Drop
        )
        && a.bars
            .iter()
            .filter(|bar| {
                bar.end_sec > ga.sec(out - ga.meter()) + 0.01 && bar.start_sec < end - 0.01
            })
            .all(|bar| bar.kick_salience >= 0.55 && crate::vocals::risk(bar) < 0.35);
    if !explicit_out && crate::arrangement::breaks_build(a, end, b, if blend { b_end } else { bin })
    {
        return miss!();
    }
    // A released instrumental recovery can carry a short native-rate overlap
    // even when incompatible tempi rule out a beat blend. Require measured
    // voice coverage throughout; a section label alone cannot authorize it.
    let short_overlap = !blend
        && !loop_roll
        && !boundary_only
        && n == 1.
        && recovery
        && !a.moments.is_empty()
        && (0..32).all(|i| {
            let sec = start + (end - start) * (i as f32 + 0.5) / 32.;
            crate::continuity::moment(a, sec).is_some()
                && crate::continuity::voice_active(a, sec) < 0.35
        });
    let instant = !short_overlap
        && !blend
        && !loop_roll
        && n == 1.
        && !drop_cut
        && !scratch_cut
        && !spinback
        && out_phrase >= 0.6
        // Recovery quality permits overlap inside a released interval. It is
        // not independent evidence of a completed phrase for a hard cut.
        && (explicit_out || boundary_quality(a, end) >= 0.6)
        && in_phrase >= 0.6
        && crate::continuity::cut_safe(a, end, false)
        && !matches!(sa, mixless_protocol::SectionLabel::BuildUp);
    if boundary_only && !instant && !drop_cut && !spinback {
        return miss!();
    }
    // Stems can strip the outgoing rhythm on a phrase line, so a short
    // native-rate exit is available where the tempi cannot be matched.
    let stem_strip = !blend
        && !loop_roll
        && stem_pair
        && (2. ..4.).contains(&n)
        && (correction - 1.).abs() > 0.08;
    if !blend
        && !loop_roll
        && n < 4.
        && !drop_cut
        && !scratch_cut
        && !instant
        && !spinback
        && !short_overlap
        && !stem_strip
    {
        return miss!();
    }
    if drop_cut && !crate::continuity::cut_safe(a, end, true) {
        return miss!();
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
            roll,
            requested: match options.strategy {
                Some(S::Spinback) => Some(Technique::Spinback),
                Some(S::TapeStop) => Some(Technique::TapeStop),
                Some(S::BuildRoll) => Some(Technique::BuildRoll),
                _ => None,
            },
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
    // A hard cut lands after the voice has rested, never on a word still
    // sounding or just released, and never swaps one voice straight into
    // another. A drop landing needs only a beat of rest; any other cut two.
    let hard_cut = instant
        || fx_decision.technique.accent()
        || fx_decision.technique == Technique::DryCut && !blend;
    // A cut into silence is a gap, not a cut.
    if hard_cut && !explicit_in && crate::continuity::silent(b, bin, 0.5) {
        return miss!();
    }
    // The song's own last moment is not a cut: nothing is left unsung.
    if hard_cut && !explicit_out && end < a.duration_sec - 0.5 {
        let beat = 60. / outgoing_bpm;
        let rest = if fx_decision.technique.accent() {
            beat
        } else {
            2. * beat
        }
        .max(0.5);
        // Only measured voice evidence can time a breath; without it the
        // coarse phrase rules above already decided.
        let sung = |t: &TrackAnalysis, from: f32, to: f32| {
            (t.stems.is_some() || !t.moments.is_empty())
                && (0..=12)
                    .map(|i| {
                        crate::continuity::voice_active(t, from + (to - from) * i as f32 / 12.)
                    })
                    .fold(0., f32::max)
                    > 0.35
        };
        if !chop_a && sung(a, end - rest, end - 0.1)
            || (!fx_decision.technique.accent()
                && sung(a, end - 2., end - 0.1)
                && sung(b, bin + 0.02, bin + 1.))
        {
            return miss!();
        }
    }
    if !explicit_out
        && !carry
        && !crate::drops::allows(
            a,
            start,
            end,
            options.outgoing_entry_sec,
            filtered || instant || fx_decision.technique.accent(),
        )
    {
        return miss!();
    }
    // Releasing A's build into B's drop only works on a shared beat; across
    // a tempo gap the groove breaks at the moment of release. A transport
    // accent stops A first, so the drop lands on silence instead.
    if leaving_into_drop
        && ((correction - 1.).abs() > 0.08 || ga.meter() != gb.meter())
        && !matches!(
            fx_decision.technique,
            Technique::Spinback | Technique::TapeStop | Technique::ScratchCut
        )
    {
        return miss!();
    }
    if (loop_roll && fx_decision.technique != Technique::LoopRoll)
        || (options.strategy == Some(S::DropCut) && fx_decision.technique != Technique::DropCut)
        || (options.strategy == Some(S::ScratchCut)
            && fx_decision.technique != Technique::ScratchCut)
        || (options.strategy == Some(S::Spinback) && fx_decision.technique != Technique::Spinback)
        || (options.strategy == Some(S::TapeStop) && fx_decision.technique != Technique::TapeStop)
        || (options.strategy == Some(S::BuildRoll) && fx_decision.technique != Technique::BuildRoll)
        || (options.strategy == Some(S::LoopOut) && !loop_out)
    {
        return miss!();
    }
    // An overlap that opens inside A's peak layers B underneath it, and the
    // bass changes hands on the peak's last bar line, the way a DJ brings
    // the next intro in over the last phrases of a drop. B must carry a beat
    // there so the swap reveals a groove, not an empty intro.
    let layered = (blend && !loop_out && !filtered && !explicit_out)
        .then(|| peak_end_bar(a, &source_a, start, end, n))
        .flatten()
        .filter(|u| {
            *u >= 2.
                && feature(b, source_b.sample(*u) + 0.01).is_some_and(|f| f.kick_salience >= 0.35)
        });
    let handoff = if let Some((_, shape)) = &ride {
        shape.leave
    } else if filtered {
        n - 1.
    } else if blend {
        if loop_out {
            n / 2.
        } else if let Some(u) = layered {
            u
        } else {
            bass_handoff(a, b, &source_a, &source_b, n).or_else(|| miss!())?
        }
    } else if let Some(op) = &lanes.loop_b {
        op.off_bar
    } else {
        start_b
    };
    // Two tracks may share a peak, but the outgoing one keeps its weight
    // until its own peak has finished: the bass changes hands after it.
    // Nor does A open a new peak underneath B once it has handed over.
    if (blend || loop_roll)
        && !carry
        && !explicit_out
        && (crate::drops::inside_peak(a, source_a.sample(handoff))
            || crate::drops::peaks(a)
                .iter()
                .any(|p| p.0 > source_a.sample(handoff) + 0.05 && p.0 < end - 0.05))
    {
        return miss!();
    }
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
            // B takes over immediately in a bridge. Later loud bars cannot
            // make its first, fading-in bar sound established at the handoff.
            gb.sec(input + gb.meter()).min(b.duration_sec),
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
        spinback: matches!(
            fx_decision.technique,
            Technique::Spinback | Technique::TapeStop | Technique::BuildRoll
        ),
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
    // A carry lets the line finish instead of fading it under the next
    // track, in proportion to how well the incoming bed carries it.
    let score = score + ride.as_ref().map_or(0., |(fit, _)| 0.04 + 0.12 * fit);
    if score <= minimum_score {
        return miss!();
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
            brake: false,
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
            return miss!();
        }
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame,
            peak_delta_frames: -pull,
            on_bar,
            peak_bar: n - 0.02,
            off_bar: n,
            accelerate: true,
            brake: false,
        });
    }
    if fx_decision.technique == Technique::TapeStop {
        // The last half bar of A brakes evenly to a standstill just before
        // B's downbeat; braking from unity covers half the normal distance.
        let on_bar = (n - 0.5).max(0.);
        let peak_bar = n - 0.02;
        let start = source_a.sample(on_bar);
        let travel = 0.5 * (source_a.sample(peak_bar) - start) * a.sample_rate as f32;
        if travel <= 0. {
            return miss!();
        }
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame: (start * a.sample_rate as f32).round().max(0.) as u64,
            peak_delta_frames: travel.round() as i64,
            on_bar,
            peak_bar,
            off_bar: n,
            accelerate: false,
            brake: true,
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
                    Technique::TapeStop => S::TapeStop,
                    Technique::BuildRoll => S::BuildRoll,
                    Technique::FilterBridge => S::FilterSweep,
                    _ => S::EchoOut,
                }
            },
            score,
            used_fallback: !blend
                && !loop_roll
                && !fx_decision.technique.accent()
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
        send_fx: None,
        insert_a: None,
    };
    // The shaped lanes hold the swap at unity per band; combined they can
    // still dip or surge. Trim both decks so the master follows the level it
    // starts and ends at, never by more than one clean boost.
    crate::level::compensate(&mut plan, a, b);
    // Two quiet sources under a full crossfade leave a hole no attenuation
    // can fill, so a blend ending inside the outgoing track's own fade is
    // only worth taking when some overlap really carries the level.
    if (blend || loop_roll) && crate::level::worst_dip(&plan, a, b) > 9. {
        return miss!();
    }
    crate::level::sanitize(&mut plan);
    if fx_decision.technique == Technique::BuildRoll {
        // Capture the first beat of A's last bar and roll it ever tighter
        // into B's drop.
        plan.insert_a = Some(mixless_protocol::InsertFx {
            kind: mixless_protocol::FxKind::LoopRoll,
            beats: 1.,
            on_bar: (n - 1.).max(0.),
        });
    }
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
    if mode == TransitionMode::PhraseBridge && !instant && !fx_decision.technique.accent() {
        crate::bridge::expand(&mut plan, ctx, fx_decision.technique, harmonic, stem_pair)
            .or_else(|| miss!())?;
    }
    if let Some((_, shape)) = &ride {
        let blend_plan = plan.clone();
        crate::stem_mix::carry(&mut plan, shape, outgoing_bpm);
        if !crate::stem_mix::swap_holds_level(&blend_plan, &plan, a, b, n) {
            return miss!();
        }
        if let Some(summary) = plan.summary.as_mut() {
            summary.strategy = S::VocalRide;
        }
    }
    if filtered {
        crate::filtered::arrange(&mut plan, a, b).or_else(|| miss!())?;
    } else if !carry
        && !loop_out
        && !short_overlap
        && plan.stem_mix.is_none()
        && (blend || loop_roll || plan.incoming_start_bar == 0.)
    {
        crate::vocals::protect(&mut plan, a).or_else(|| miss!())?;
    }
    // A cut away from an incoming drop may let the outgoing last beat ring
    // out on a tail; that keeps its one-bar lead so the send can catch it.
    let tail = instant
        && !drop_cut
        && plan.lanes.scratch_a.is_none()
        && crate::send_fx::cut_tail(ctx, &mut plan, harmonic);
    // A scratch op needs its material: compacting would crop the pull bars.
    // Only a real cut is compacted; a bridge that ends on an incoming drop
    // keeps its bars.
    if (instant || fx_decision.technique == Technique::DropCut)
        && plan.lanes.scratch_a.is_none()
        && !tail
    {
        crate::instant::compact(&mut plan);
    }
    if plan.t_in_a < options.earliest_outgoing_sec {
        return miss!();
    }
    if stem_layer && !loop_out {
        crate::stem_mix::layer(ctx, &mut plan, handoff);
    }
    // With stems on a long harmonic blend, swap the parts on phrase lines
    // instead of crossfading whole tracks: one kit, one bass, one voice.
    if blend
        && stem_pair
        && harmonic
        && !loop_out
        && !filtered
        && !stem_layer
        && n >= 8.
        && plan.stem_mix.is_none()
    {
        let drum_bar = crate::stem_mix::drum_bar(handoff)
            .filter(|_| handoff + 2.25 <= n)
            .filter(|bar| crate::stem_mix::swap_supported(&plan, a, b, *bar));
        if let Some(drum_bar) = drum_bar {
            let ride = crate::stem_mix::ride_supported(&plan, a, b, n)
                .filter(|end| *end > handoff + 1. && *end <= n - 1.5);
            let voice = crate::stem_mix::voice_points(&plan, a, b, ride.unwrap_or(handoff), n);
            let swapped = voice.map(|voice| {
                let mut swapped = plan.clone();
                crate::stem_mix::swap(&mut swapped, handoff, drum_bar, voice, ride.is_some());
                swapped
            });
            if let Some(swapped) =
                swapped.filter(|swapped| crate::stem_mix::swap_holds_level(&plan, swapped, a, b, n))
            {
                plan = swapped;
                if let Some(summary) = plan.summary.as_mut() {
                    summary.strategy = if ride.is_some() {
                        S::VocalRide
                    } else {
                        S::StemSwap
                    };
                }
            }
        }
    }
    if !harmonic && plan.requires_stems {
        crate::stem_mix::separate_tonal(&mut plan).or_else(|| miss!())?;
    }
    Some(plan)
}

/// The bar, within the overlap, on which an A peak that the overlap opens
/// inside comes to its end.
fn peak_end_bar(
    a: &TrackAnalysis,
    source_a: &Polyline,
    start: f32,
    end: f32,
    n: f32,
) -> Option<f32> {
    let (_, peak_end) = crate::drops::peaks(a)
        .into_iter()
        .find(|p| p.0 < start - 0.05 && p.1 > start + 0.05 && p.1 <= end + 0.08)?;
    (1..=n as usize)
        .map(|bar| bar as f32)
        .find(|u| (source_a.sample(*u) - peak_end).abs() < 0.08)
}
