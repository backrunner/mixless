//! Predicted master loudness of a plan, band by band. It combines the
//! measured band levels of each source with every lane the engine applies:
//! the equal-power crossfader, channel gain, the three-band isolator, the
//! channel filters and any stem gains. The mix overlay, the stem swap and
//! the gain compensation all judge a transition by the same estimate.
use mixless_protocol::{MixPlan, Polyline, StemMix, TrackAnalysis};

/// Representative centres of the isolator's bands (crossovers at 150 Hz and
/// 2 kHz) for the filter responses, Hz.
const CENTRES: [f32; 3] = [70., 600., 5000.];

fn lin(db: f32) -> f32 {
    if db <= -90. { 0. } else { 10f32.powf(db / 20.) }
}

/// Second-order magnitude of the channel filters at `hz`.
fn filter_gain(lp: f32, hp: f32, hz: f32) -> f32 {
    let low = 1. / (1. + (hz / lp.max(1.)).powi(4)).sqrt();
    let high = 1. / (1. + (hp.max(1.) / hz).powi(4)).sqrt();
    low * high
}

/// Band powers of one source around `sec`: low, mid, high in linear power.
/// Stem evidence splits each band into vocals, drums and instruments (the
/// bass stem is part of instruments in the meters).
fn bands(t: &TrackAnalysis, sec: f32, stems: Option<[f32; 3]>) -> [f32; 3] {
    if let Some(meters) = &t.stems {
        let frames: Vec<_> = (0..4)
            .filter_map(|i| meters.frame_at(sec - 0.15 + 0.1 * i as f32))
            .collect();
        if !frames.is_empty() {
            let mut power = [0f32; 3];
            for f in &frames {
                for (stem, gains) in f.band_db.iter().enumerate() {
                    let g = stems.map_or(1., |s| s[stem]);
                    for band in 0..3 {
                        power[band] += 10f32.powf(gains[band] / 10.) * g * g;
                    }
                }
            }
            return power.map(|p| p / frames.len() as f32);
        }
    }
    let Some(bar) = crate::musical::feature(t, sec) else {
        return [0.; 3];
    };
    let raw = [bar.low_db, bar.mid_db, bar.high_db].map(|db| 10f32.powf(db / 10.));
    let total = raw.iter().sum::<f32>().max(1e-12);
    raw.map(|p| p / total * bar.rms * bar.rms)
}

/// Stem gains for each analysed part, folding bass into instruments.
fn parts(mix: &[Polyline; 4], u: f32) -> [f32; 3] {
    let g = |i: usize| mix[i].sample(u).clamp(0., 1.);
    [g(0), g(1), ((g(2).powi(2) + g(3).powi(2)) * 0.5).sqrt()]
}

/// Predicted master level in dB at bar `u` of the plan.
pub(crate) fn at(plan: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis, u: f32) -> f32 {
    let l = &plan.lanes;
    let x = (l.xfader.sample(u).clamp(-1., 1.) + 1.) * 0.5;
    let stems: Option<&StemMix> = plan.stem_mix.as_ref();
    let deck = |t: &TrackAnalysis,
                source: &Polyline,
                fader: f32,
                gain: &Polyline,
                eq: &mixless_protocol::EqLane,
                filter: &mixless_protocol::FilterLane,
                stem: Option<&[Polyline; 4]>|
     -> [f32; 3] {
        if source.nodes.is_empty() || fader <= 0. {
            return [0.; 3];
        }
        let p = bands(t, source.sample(u), stem.map(|s| parts(s, u)));
        let g = fader * lin(gain.sample(u));
        let eqs = [eq.low.sample(u), eq.mid.sample(u), eq.high.sample(u)].map(lin);
        let (lp, hp) = (filter.lp_hz.sample(u), filter.hp_hz.sample(u));
        std::array::from_fn(|i| {
            let f = filter_gain(lp, hp, CENTRES[i]);
            p[i] * (g * eqs[i] * f).powi(2)
        })
    };
    let a_bands = deck(
        a,
        &plan.outgoing_source,
        (1. - x).sqrt(),
        &l.gain_a,
        &l.eq_a,
        &l.filter_a,
        stems.map(|s| &s.outgoing),
    );
    let b_bands = if u + 0.0001 >= plan.incoming_start_bar {
        deck(
            b,
            &plan.incoming_source,
            x.sqrt(),
            &l.gain_b,
            &l.eq_b,
            &l.filter_b,
            stems.map(|s| &s.incoming),
        )
    } else {
        [0.; 3]
    };
    let total: f32 = a_bands.iter().chain(&b_bands).sum();
    10. * total.max(1e-12).log10()
}

/// Hold the master near the straight line between the plan's own level at
/// its two ends, by attenuating only: a transition never gets louder than
/// the decks it mixes, so a quiet source is never lifted to match a loud
/// one. Surges where a bass exchange or a stem overlap adds power are
/// pulled back to the line.
pub(crate) fn compensate(plan: &mut MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) {
    const STEP_BARS: f32 = 0.25;
    const SLACK_DB: f32 = 1.5;
    let Some(summary) = &plan.summary else { return };
    let n = summary.length_bars as f32;
    if n <= 0. || plan.outgoing_source.nodes.is_empty() || plan.incoming_source.nodes.is_empty() {
        return;
    }
    let start = at(plan, a, b, 0.);
    let finish = at(plan, a, b, n);
    if !start.is_finite() || !finish.is_finite() {
        return;
    }
    // One correction per step, held flat only within the step it measures,
    // so the curve returns to zero wherever the level is back on the line.
    let steps = (n / STEP_BARS).ceil().max(1.) as usize;
    let correction: Vec<f32> = (0..=steps)
        .map(|i| {
            let u = n * i as f32 / steps as f32;
            let target = start + (finish - start) * i as f32 / steps as f32;
            let offset = (target - at(plan, a, b, u)).clamp(-12., 0.);
            if offset < -SLACK_DB { offset } else { 0. }
        })
        .collect();
    if correction.iter().all(|c| *c == 0.) {
        return;
    }
    let at_u = |u: f32| {
        let x = (u / n * steps as f32).clamp(0., steps as f32);
        let (lo, hi) = (x.floor() as usize, (x.ceil() as usize).min(steps));
        let t = x - lo as f32;
        correction[lo] + (correction[hi] - correction[lo]) * t
    };
    for gain in [&mut plan.lanes.gain_a, &mut plan.lanes.gain_b] {
        let mut nodes: Vec<(f32, f32)> = gain
            .nodes
            .iter()
            .map(|(u, v)| (*u, (v + at_u(*u)).clamp(-96., 6.)))
            .collect();
        if nodes.first().is_none_or(|(u, _)| *u > 0.) {
            nodes.insert(0, (0., (gain.sample(0.) + correction[0]).clamp(-96., 6.)));
        }
        nodes.sort_by(|x, y| x.0.total_cmp(&y.0));
        nodes.dedup_by(|x, y| x.0 == y.0);
        gain.nodes = nodes;
    }
}

/// The deepest hole in a transition, in dB below the straight line between
/// the level each source plays at on its own. A transition that drops far
/// below that line leaves a gap a listener hears as the mix disappearing.
pub(crate) fn worst_dip(plan: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    const STEP_BARS: f32 = 0.25;
    let Some(summary) = &plan.summary else {
        return 0.;
    };
    let n = summary.length_bars as f32;
    if n <= 0. || plan.outgoing_source.nodes.is_empty() || plan.incoming_source.nodes.is_empty() {
        return 0.;
    }
    let start = at(plan, a, b, 0.);
    let finish = at(plan, a, b, n);
    if !start.is_finite() || !finish.is_finite() {
        return 0.;
    }
    let steps = (n / STEP_BARS).ceil().max(1.) as usize;
    (0..=steps)
        .map(|i| {
            let u = n * i as f32 / steps as f32;
            let target = start + (finish - start) * i as f32 / steps as f32;
            target - at(plan, a, b, u)
        })
        .fold(0., f32::max)
}

/// Clamp every lane to the range the engine accepts. Curve evaluation of an
/// eased blend can land a hair outside the limit (1.0000007 on a crossfader),
/// which the engine treats as an invalid plan rather than a rounding artefact.
pub(crate) fn sanitize(plan: &mut MixPlan) {
    let clamp = |line: &mut Polyline, lo: f32, hi: f32| {
        for (_, v) in &mut line.nodes {
            if !v.is_finite() {
                *v = lo;
            } else {
                *v = v.clamp(lo, hi);
            }
        }
    };
    let l = &mut plan.lanes;
    clamp(&mut l.xfader, -1., 1.);
    for line in [
        &mut l.gain_a,
        &mut l.gain_b,
        &mut l.eq_a.low,
        &mut l.eq_a.mid,
        &mut l.eq_a.high,
        &mut l.eq_b.low,
        &mut l.eq_b.mid,
        &mut l.eq_b.high,
    ] {
        clamp(line, -96., 12.);
    }
    clamp(&mut l.fx_send_a, 0., 1.);
    clamp(&mut l.fx_send_b, 0., 1.);
    clamp(&mut l.rate_a, 0.5, 2.);
    clamp(&mut l.rate_b, 0.5, 2.);
    clamp(&mut l.pitch_a, -24., 24.);
    clamp(&mut l.pitch_b, -24., 24.);
    for line in [
        &mut l.filter_a.lp_hz,
        &mut l.filter_a.hp_hz,
        &mut l.filter_b.lp_hz,
        &mut l.filter_b.hp_hz,
    ] {
        clamp(line, 20., 20000.);
    }
    if let Some(stems) = &mut plan.stem_mix {
        for line in stems.outgoing.iter_mut().chain(stems.incoming.iter_mut()) {
            clamp(line, 0., 1.);
        }
    }
}
