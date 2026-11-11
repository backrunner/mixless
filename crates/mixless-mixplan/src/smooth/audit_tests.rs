//! Opt-in, read-only real-corpus enumeration without score pruning.
use super::*;

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT and MIXLESS_AUDIT_PLAN"]
fn inspect_transition_continuity() {
    let read = |key| std::fs::read(std::env::var(key).unwrap()).unwrap();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&read("MIXLESS_AUDIT_INPUT")).unwrap();
    let p: MixPlan = serde_json::from_slice(&read("MIXLESS_AUDIT_PLAN")).unwrap();
    let (a, b) = (&tracks[0], &tracks[1]);
    let c = crate::experience::continuity(&p, a, b).unwrap();
    eprintln!(
        "CONTINUITY {}",
        serde_json::json!({"pair":p.summary.as_ref().unwrap().pair,
        "gap_bars":c.foreground_gap_bars,"longest_gap_bars":c.longest_foreground_gap_bars,
        "minimum_bar_retention":crate::experience::minimum_bar_foreground_retention(&p,a,b),
        "terminal":crate::experience::terminal_handoff(&p,a,b),
        "needs_repair":crate::experience::needs_repair(&p,a,b)})
    );
}

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT, AUDIT_PREVIOUS_PLAN and MIXLESS_AUDIT_PLAN"]
fn inspect_entrance_forecasts() {
    let read = |key| std::fs::read(std::env::var(key).unwrap()).unwrap();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&read("MIXLESS_AUDIT_INPUT")).unwrap();
    let previous: MixPlan = serde_json::from_slice(&read("AUDIT_PREVIOUS_PLAN")).unwrap();
    let stored: MixPlan = serde_json::from_slice(&read("MIXLESS_AUDIT_PLAN")).unwrap();
    let ctx = PlanContext {
        outgoing: &tracks[0],
        incoming: &tracks[1],
        cues_out: &[],
        cues_in: &[],
        offset_a: previous.incoming_offset_end,
        offset_b: Default::default(),
    };
    let planner = crate::Planner::with_options(PlannerOptions {
        stem_playback: true,
        harmonic_key_shift: true,
        outgoing_entry_sec: crate::established_entry(&previous, &tracks[0]),
        earliest_outgoing_sec: previous.t_end_b + previous.incoming_offset_end.rate,
        ..Default::default()
    });
    let mut choices = planner.plan_choices(&ctx, true);
    let shortlist_len = choices.len();
    choices.push(planner.finish_plan(&ctx, stored));
    for (index, p) in choices.iter().enumerate() {
        let next = crate::pitch_recovery::departure(
            &planner,
            &tracks[1],
            &tracks[2],
            p,
            p.incoming_offset_end.pitch_semitones,
            &[],
            &[],
        );
        let current = crate::order::audible_cost(p, &tracks[0], &tracks[1], 0., 0.);
        let future = next.as_ref().map_or(1.5, |q| {
            crate::order::audible_cost(q, &tracks[1], &tracks[2], 0., 0.)
        });
        eprintln!(
            "FORECAST {}",
            serde_json::json!({"shortlisted":index<shortlist_len,
            "plan":p,"entry":crate::established_entry(p,&tracks[1]),
            "owed":crate::drops::owed_peak(&tracks[1],crate::established_entry(p,&tracks[1])),
            "current":current,"future":future,"total":current+0.5*future,"next":next})
        );
    }
}

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT, MIXLESS_AUDIT_PLAN and MIXLESS_AUDIT_OUTPUT"]
fn inspect_vocal_controls() {
    let read = |key| std::fs::read(std::env::var(key).unwrap()).unwrap();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&read("MIXLESS_AUDIT_INPUT")).unwrap();
    let p: MixPlan = serde_json::from_slice(&read("MIXLESS_AUDIT_PLAN")).unwrap();
    let _scope = crate::vocals::pattern::Scope::new(&tracks[0], &tracks[1]);
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    let mut rows = Vec::new();
    for (side, track) in tracks.iter().take(2).enumerate() {
        let (gain, eq, filter, stems) = if side == 0 {
            (
                &p.lanes.gain_a,
                &p.lanes.eq_a,
                &p.lanes.filter_a,
                p.stem_mix.as_ref().map(|s| &s.outgoing),
            )
        } else {
            (
                &p.lanes.gain_b,
                &p.lanes.eq_b,
                &p.lanes.filter_b,
                p.stem_mix.as_ref().map(|s| &s.incoming),
            )
        };
        let mut previous = None::<f32>;
        for i in 0..=(n * 32.) as usize {
            let u = i as f32 / 32.;
            let sec = p.source_seconds_at(side == 1, u);
            let x = ((p.lanes.xfader.sample(u) + 1.) * 0.5).clamp(0., 1.);
            let amplitude = (if side == 0 { 1. - x } else { x }).sqrt()
                * stems.map_or(1., |s| s[0].sample(u))
                * 10f32.powf((gain.sample(u) + eq.mid.sample(u)) / 20.)
                / ((1. + (600. / filter.lp_hz.sample_log(u).max(20.)).powi(4))
                    * (1. + (filter.hp_hz.sample_log(u).max(20.) / 600.).powi(4)))
                .sqrt();
            if previous.is_none_or(|old| (amplitude - old).abs() > 0.0001)
                && (0.01..0.95).contains(&amplitude)
            {
                let frame = track.stems.as_ref().and_then(|s| s.frame_at(sec));
                rows.push(serde_json::json!({"side":side,"source":sec,"bar":u,
                    "amplitude":amplitude,"activity":frame.map(|f|f.vocal_activity),
                    "continuation":format!("{:?}",crate::vocals::continuation(track,sec)),
                    "held_note":crate::vocals::held_note(track,sec),
                    "pattern":crate::vocals::pattern::at(track,sec).is_some()}));
            }
            previous = Some(amplitude);
        }
    }
    std::fs::write(
        std::env::var("MIXLESS_AUDIT_OUTPUT").unwrap(),
        serde_json::to_vec(&rows).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT and MIXLESS_AUDIT_OUTPUT; optional AUDIT_PREVIOUS_PLAN"]
fn enumerate_feasible_arrangements() {
    let input = std::env::var("MIXLESS_AUDIT_INPUT").unwrap();
    let output = std::env::var("MIXLESS_AUDIT_OUTPUT").unwrap();
    let tracks: Vec<TrackAnalysis> =
        serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let previous: Option<MixPlan> = std::env::var("AUDIT_PREVIOUS_PLAN")
        .ok()
        .map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap());
    let ctx = PlanContext {
        outgoing: &tracks[0],
        incoming: &tracks[1],
        cues_out: &[],
        cues_in: &[],
        offset_a: previous
            .as_ref()
            .map_or(Default::default(), |p| p.incoming_offset_end),
        offset_b: Default::default(),
    };
    let options = PlannerOptions {
        stem_playback: true,
        harmonic_key_shift: true,
        outgoing_entry_sec: previous
            .as_ref()
            .map_or(0., |p| crate::established_entry(p, ctx.outgoing)),
        earliest_outgoing_sec: previous
            .as_ref()
            .map_or(0., |p| p.t_end_b + p.incoming_offset_end.rate),
        ..Default::default()
    };
    let _peaks = mixless_protocol::PeakScope::new(ctx.outgoing, ctx.incoming);
    let _instruments = mixless_protocol::InstrumentScope::new(ctx.outgoing, ctx.incoming);
    let _audible = crate::experience::Scope::new(ctx.outgoing, ctx.incoming);
    let _vocals = crate::vocals::pattern::Scope::new(ctx.outgoing, ctx.incoming);
    eprintln!(
        "Entry {} earliest {} recovery {:?} owed {:?}",
        options.outgoing_entry_sec,
        options.earliest_outgoing_sec,
        crate::recovery::windows(ctx.outgoing),
        crate::drops::owed_peak(ctx.outgoing, options.outgoing_entry_sec)
    );
    eprintln!(
        "Exit candidates {:?}",
        points(ctx.outgoing, &[], true, options.earliest_outgoing_sec)
            .into_iter()
            .map(|b| {
                let sec = Grid(ctx.outgoing).sec(b);
                (
                    sec,
                    crate::drops::allows(ctx.outgoing, sec, sec, options.outgoing_entry_sec, true),
                    crate::vocals::safe_exit(ctx.outgoing, sec),
                )
            })
            .collect::<Vec<_>>()
    );
    let mut plans = Vec::new();
    for out in points(ctx.outgoing, &[], true, options.earliest_outgoing_sec) {
        if std::env::var("MIXLESS_AUDIT_OUT_FROM")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .is_some_and(|from| Grid(ctx.outgoing).sec(out) < from)
        {
            continue;
        }
        for input in points(ctx.incoming, &[], false, 0.) {
            for mode in [
                TransitionMode::BeatBlend,
                TransitionMode::LoopRoll,
                TransitionMode::PhraseBridge,
            ] {
                let lengths = if std::env::var_os("MIXLESS_AUDIT_ALL_LENGTHS").is_some()
                    && mode == TransitionMode::BeatBlend
                {
                    (4..=128).collect()
                } else {
                    crate::duration::candidates(&ctx, &options, out, input, mode)
                };
                for length in lengths {
                    if let Some(p) = pair(&ctx, &options, out, input, mode, length, -1.) {
                        plans.push(p);
                    }
                }
            }
        }
        eprintln!(
            "Exit {:.3}: {:?}",
            Grid(ctx.outgoing).sec(out),
            crate::trace::take()
        );
    }
    plans.sort_by(|a, b| {
        b.summary
            .as_ref()
            .unwrap()
            .score
            .total_cmp(&a.summary.as_ref().unwrap().score)
    });
    for p in &plans {
        eprintln!(
            "FEASIBLE {}",
            serde_json::json!({
                "summary":p.summary,"out":p.t_out_a,"in":p.t_in_b,
                "entry":crate::established_entry(p,ctx.incoming),
                "owed":crate::drops::owed_peak(ctx.incoming,crate::established_entry(p,ctx.incoming)),
                "merit":crate::experience::merit(p,ctx.outgoing,ctx.incoming)
            })
        );
    }
    eprintln!(
        "{} feasible arrangements; rejections {:?}",
        plans.len(),
        crate::trace::take()
    );
    std::fs::write(output, serde_json::to_vec(&plans).unwrap()).unwrap();
}

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT"]
fn diagnose_sequential_boundaries() {
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(
        &std::fs::read(std::env::var("MIXLESS_AUDIT_INPUT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut offset = Default::default();
    let mut entry = 0.;
    let mut earliest = 0.;
    for (i, pair) in tracks.windows(2).take(5).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let ctx = PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: &[],
            cues_in: &[],
            offset_a: offset,
            offset_b: Default::default(),
        };
        let options = PlannerOptions {
            stem_playback: true,
            harmonic_key_shift: true,
            outgoing_entry_sec: entry,
            earliest_outgoing_sec: earliest,
            ..Default::default()
        };
        eprintln!(
            "PAIR {i}: entry={entry:.3} earliest={earliest:.3}, peaks={:?}, owed={:?}",
            crate::drops::peaks(a),
            crate::drops::owed_peak(a, entry)
        );
        eprintln!(
            "OUTS {:?}",
            points(a, &[], true, earliest)
                .into_iter()
                .map(|p| {
                    let sec = Grid(a).sec(p);
                    (
                        sec,
                        crate::drops::allows(a, sec, sec, entry, true),
                        crate::vocals::safe_exit(a, sec),
                    )
                })
                .collect::<Vec<_>>()
        );
        eprintln!(
            "INS {:?}",
            points(b, &[], false, 0.)
                .into_iter()
                .map(|p| {
                    let sec = Grid(b).sec(p);
                    (
                        sec,
                        boundary_quality(b, sec),
                        crate::drops::entry_allowed(b, sec),
                    )
                })
                .collect::<Vec<_>>()
        );
        let mut p =
            crate::Planner::with_options(options).plan_with_following(&ctx, tracks.get(i + 2));
        if p.failure_reason.is_some() {
            p = crate::short_handoff(&ctx, earliest);
        }
        eprintln!(
            "PLAN {:?} {:.3}..{:.3} -> {:.3}..{:.3}",
            p.summary, p.t_in_a, p.t_out_a, p.t_in_b, p.t_end_b
        );
        offset = p.incoming_offset_end;
        entry = crate::established_entry(&p, b);
        earliest = p.t_end_b + offset.rate;
    }
}

#[test]
#[ignore = "requires MIXLESS_AUDIT_INPUT, MIXLESS_AUDIT_PLAN and AUDIT_PREVIOUS_PLAN"]
fn replay_stored_window() {
    let read = |key| std::fs::read(std::env::var(key).unwrap()).unwrap();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&read("MIXLESS_AUDIT_INPUT")).unwrap();
    let old: MixPlan = serde_json::from_slice(&read("MIXLESS_AUDIT_PLAN")).unwrap();
    let previous: MixPlan = serde_json::from_slice(&read("AUDIT_PREVIOUS_PLAN")).unwrap();
    let ctx = PlanContext {
        outgoing: &tracks[0],
        incoming: &tracks[1],
        cues_out: &[],
        cues_in: &[],
        offset_a: previous.incoming_offset_end,
        offset_b: Default::default(),
    };
    let _scope = crate::vocals::pattern::Scope::new(ctx.outgoing, ctx.incoming);
    let options = PlannerOptions {
        stem_playback: true,
        harmonic_key_shift: true,
        outgoing_entry_sec: crate::established_entry(&previous, ctx.outgoing),
        earliest_outgoing_sec: previous.t_end_b + previous.incoming_offset_end.rate,
        ..Default::default()
    };
    eprintln!(
        "Stored foreground preserved={} uncertainty={} energy_loss={}",
        crate::foreground::preserved(&old, &ctx),
        crate::foreground::uncertainty(&old, &ctx),
        crate::energy::loss_cost(&old, ctx.outgoing, ctx.incoming)
    );
    let out = Grid(ctx.outgoing).beat(old.t_out_a);
    let input = Grid(ctx.incoming).beat(old.t_in_b);
    let result = pair(
        &ctx,
        &options,
        out,
        input,
        old.transition_mode.unwrap(),
        old.summary.as_ref().unwrap().length_bars,
        -1.,
    );
    eprintln!(
        "Replayed {:?}; rejection sites {:?}",
        result.as_ref().and_then(|p| p.summary.as_ref()),
        crate::trace::take()
    );
    if let Ok(path) = std::env::var("MIXLESS_AUDIT_OUTPUT") {
        std::fs::write(path, serde_json::to_vec(&result).unwrap()).unwrap();
    }
}
