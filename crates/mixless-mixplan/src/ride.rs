//! Whether an outgoing vocal may ride over the incoming instrumental, and how
//! it leaves. The voice was written over its own arrangement, so the incoming
//! bed has to move with it: grooves on together, energy rising and falling
//! together, and no harmony that fights the line harder than the original did.
use crate::musical::feature;
use mixless_protocol::{Polyline, StemKind, TrackAnalysis};

const DISSONANCE: [f32; 12] = [
    0., 1., 0.65, 0.12, 0.1, 0.2, 0.85, 0.05, 0.1, 0.12, 0.65, 1.,
];

/// The level of a track's loud bars, so energy compares across masters.
fn usual(t: &TrackAnalysis) -> f32 {
    let mut levels: Vec<f32> = t
        .bars
        .iter()
        .map(|b| b.rms)
        .filter(|r| *r > 0.001)
        .collect();
    levels.sort_by(f32::total_cmp);
    levels
        .get(levels.len().saturating_sub(1) * 4 / 5)
        .copied()
        .unwrap_or(0.)
}

fn correlation(x: &[f32], y: &[f32]) -> f32 {
    let n = x.len().min(y.len()) as f32;
    if n < 3. {
        return 0.;
    }
    let (mx, my) = (x.iter().sum::<f32>() / n, y.iter().sum::<f32>() / n);
    let (mut sxy, mut sx, mut sy) = (0., 0., 0.);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sx += (a - mx).powi(2);
        sy += (b - my).powi(2);
    }
    if sx < 1e-6 || sy < 1e-6 {
        0.
    } else {
        sxy / (sx * sy).sqrt()
    }
}

/// Mean dissonance of the sung notes at `sec` in `a` against the sounding
/// note chroma of `bed` at `bed_sec`. None where either side is silent.
fn sung_cost(
    a: &TrackAnalysis,
    sec: f32,
    bed: &TrackAnalysis,
    bed_sec: f32,
    voice_pitch: f32,
    bed_pitch: f32,
) -> Option<f32> {
    let notes: Vec<u8> = a
        .stems
        .as_ref()?
        .notes
        .iter()
        .filter(|n| {
            n.stem == StemKind::Vocals
                && n.confidence >= 0.45
                && n.start_sec <= sec
                && n.end_sec > sec
        })
        .map(|n| n.midi)
        .collect();
    let chroma = bed.stems.as_ref()?.frame_at(bed_sec)?.note_chroma;
    let mass: f32 = chroma.iter().map(|v| v * v).sum();
    if notes.is_empty() || chroma.iter().all(|v| *v < 0.1) || mass <= 0. {
        return None;
    }
    let cost: f32 = notes
        .iter()
        .flat_map(|m| (0..12).map(move |pc| (*m, pc)))
        .map(|(m, pc)| {
            let interval = (m as f32 + voice_pitch - pc as f32 - bed_pitch).rem_euclid(12.);
            let low = interval.floor() as usize;
            let mix = interval.fract();
            (DISSONANCE[low] * (1. - mix) + DISSONANCE[(low + 1) % 12] * mix) * chroma[pc].powi(2)
        })
        .sum();
    Some(cost / (mass * notes.len() as f32))
}

/// How well B's bed carries A's voice over the `n` aligned bars, from 0 to
/// 1, or None when it does not. Judged on A's own arrangement, which the
/// line was written over, against what B plays underneath at the same time.
/// `harmonic` is the measured key match, used where no notes were heard.
pub(crate) fn fit(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    source_a: &Polyline,
    source_b: &Polyline,
    n: f32,
    harmonic: bool,
    pitch_a: f32,
    pitch_b: f32,
) -> Option<f32> {
    // Local note consonance cannot substitute for a compatible key across
    // the complete sung phrase.
    if !harmonic {
        return None;
    }
    let (ua, ub) = (usual(a), usual(b));
    let bars = n.floor() as usize;
    if ua <= 0. || ub <= 0. || bars < 8 {
        return None;
    }
    let mut agree = Vec::with_capacity(bars);
    let (mut ea, mut eb) = (vec![], vec![]);
    for i in 0..bars {
        let u = i as f32 + 0.5;
        let (Some(fa), Some(fb)) = (
            feature(a, source_a.sample(u)),
            feature(b, source_b.sample(u)),
        ) else {
            agree.push(0.);
            continue;
        };
        let (xa, xb) = (fa.rms / ua, fb.rms / ub);
        // A sung line keeps the pulse of the groove it was written to.
        let groove = (fa.kick_salience >= 0.5) == (fb.kick_salience >= 0.5);
        agree.push((1. - (xa - xb).abs().min(1.)) * if groove { 1. } else { 0.2 });
        ea.push(xa);
        eb.push(xb);
    }
    // No phrase may put the voice on a bed that does the opposite of what
    // its own arrangement did there.
    let worst = agree
        .windows(8)
        .map(|w| w.iter().sum::<f32>() / 8.)
        .fold(f32::MAX, f32::min);
    if worst < 0.25 {
        return None;
    }
    // Both arrangements head the same way: building, holding or falling.
    let phrase = |v: &[f32]| -> Vec<f32> {
        v.chunks(4)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    };
    if bars >= 16 && correlation(&phrase(&ea), &phrase(&eb)) < 0. {
        return None;
    }
    // Harmony is judged against how the voice sits on its own bed, so a
    // line that leans on tension in the original is not held to more.
    let (mut over, mut own, mut count) = (0., 0., 0);
    for j in 0..bars * 8 {
        let u = j as f32 / 8.;
        let (sa, sb) = (source_a.sample(u), source_b.sample(u));
        if let (Some(x), Some(y)) = (
            sung_cost(a, sa, b, sb, pitch_a, pitch_b),
            sung_cost(a, sa, a, sa, pitch_a, pitch_a),
        ) {
            over += x;
            own += y;
            count += 1;
        }
    }
    let excess = if count >= 32 {
        (over - own) / count as f32
    } else if harmonic {
        0.
    } else {
        return None;
    };
    if excess > 0.3 {
        return None;
    }
    let mean = agree.iter().sum::<f32>() / agree.len() as f32;
    Some((mean * (1. - excess.max(0.))).clamp(0., 1.))
}

/// How the riding voice leaves, chosen from the way its line ends and what
/// the incoming track does at that moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Exit {
    /// The line decays on its own: follow it down with a little space.
    Tail,
    /// The line stops dead: throw the last word into a beat echo.
    Throw,
    /// The incoming drop lands on the release: thin the voice out of its way.
    Sweep,
    /// The incoming bed is sparse: let the voice dissolve into a long reverb.
    Wash,
}

pub(crate) struct Shape {
    /// Bar by which the outgoing kit, bass and tonal layers have gone.
    pub leave: f32,
    /// Bar where the last sung word ends.
    pub voice_end: f32,
    pub exit: Exit,
    /// Voice level while riding, matched to the incoming bed's energy.
    pub sit_db: f32,
    /// How far below unity the incoming deck starts, so it arrives at the
    /// outgoing track's present level instead of its own.
    pub entry_db: f32,
}

pub(crate) fn shape(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    source_a: &Polyline,
    source_b: &Polyline,
    n: f32,
    locked: bool,
) -> Shape {
    let step = 1. / 16.;
    let voice_end = (0..=(n * 16.) as usize)
        .rev()
        .map(|j| j as f32 * step)
        .find(|u| crate::continuity::voice_active(a, source_a.sample(*u)) > 0.35)
        .map_or(n - 1., |u| (u + step).min(n));
    // The outgoing parts leave over the first phrase, on a line both
    // arrangements recognise where there is one.
    let limit = (n / 2.).floor().max(2.);
    let leave = [8., 4., 16.]
        .into_iter()
        .filter(|u| *u <= limit)
        .find(|u| {
            crate::phrasing::boundary_quality(b, source_b.sample(*u)) >= 0.5
                || crate::phrasing::boundary_quality(a, source_a.sample(*u)) >= 0.5
        })
        .unwrap_or(limit.min(8.));
    let level = |t: &TrackAnalysis, s: &Polyline, from: f32, to: f32| {
        let count = ((to - from).max(1.)) as usize;
        let sum: f32 = (0..count)
            .filter_map(|i| feature(t, s.sample(from + i as f32 + 0.5)).map(|f| f.rms))
            .sum();
        sum / count as f32
    };
    let (ua, ub) = (usual(a).max(0.001), usual(b).max(0.001));
    let ratio = (level(b, source_b, leave, voice_end) / ub)
        / (level(a, source_a, leave, voice_end) / ua).max(0.05);
    let sit_db = 20. * ratio.clamp(0.7, 1.).log10();
    // A quiet outgoing passage meeting a loud incoming one would jump: the
    // incoming deck starts at the outgoing level and is ridden up to unity.
    let entry_db = 20.
        * ((level(b, source_b, 0., leave) / ub) / (level(a, source_a, 0., leave) / ua).max(0.05))
            .max(1.)
            .log10();
    let entry_db = entry_db.min(9.);
    // A line that is still at full voice in its last eighth of a bar was
    // cut off rather than sung out.
    let vocal = |u: f32| {
        a.stems
            .as_ref()
            .and_then(|s| s.frame_at(source_a.sample(u)))
            .map_or(0., |f| f.rms[0])
    };
    let last = (0..4)
        .map(|i| vocal(voice_end - step * (i as f32 + 0.5)))
        .sum::<f32>()
        / 4.;
    let held = (0..16)
        .map(|i| vocal(voice_end - 2. + step * 2. * i as f32))
        .fold(0f32, f32::max);
    let abrupt = held > 0.001 && last > held * 0.5;
    let release = source_b.sample(voice_end.min(n));
    let bar = (source_b.sample(n) - source_b.sample((n - 1.).max(0.))).max(0.1);
    let drop_lands = crate::drops::peaks(b)
        .iter()
        .any(|p| p.0 >= release - bar && p.0 <= source_b.sample(n) + bar);
    let sparse = feature(b, source_b.sample((n - 0.5).max(0.)))
        .is_some_and(|f| f.rms / ub < 0.6 || f.kick_salience < 0.35);
    let exit = if drop_lands {
        Exit::Sweep
    } else if sparse || (abrupt && !locked) {
        Exit::Wash
    } else if abrupt {
        Exit::Throw
    } else {
        Exit::Tail
    };
    Shape {
        leave,
        voice_end,
        exit,
        sit_db,
        entry_db,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stem_regressions::with_stems;
    use mixless_protocol::{MixPlan, MixPlanSummary, Section, SectionLabel, StrategyId, TrackId};

    fn pair() -> (TrackAnalysis, TrackAnalysis, Polyline) {
        let a = with_stems(crate::tests::track(
            1,
            120.,
            "8A",
            SectionLabel::Chorus,
            64,
            0.9,
            0.2,
        ));
        let mut b = a.clone();
        b.track_id = TrackId(2);
        // 120 BPM: two seconds a bar on both decks.
        let source = Polyline {
            nodes: vec![(0., 20.), (32., 84.)],
        };
        (a, b, source)
    }

    #[test]
    fn sung_notes_are_compared_at_both_decks_sounding_pitch() {
        let (mut a, mut b, s) = pair();
        a.stems
            .as_mut()
            .unwrap()
            .notes
            .push(mixless_protocol::StemNote {
                stem: StemKind::Vocals,
                start_sec: 0.,
                end_sec: a.duration_sec,
                midi: 60,
                confidence: 0.95,
            });
        for frame in &mut b.stems.as_mut().unwrap().frames {
            frame.note_chroma = std::array::from_fn(|pc| if pc == 1 { 1. } else { 0. });
        }
        assert!(fit(&a, &b, &s, &s, 32., true, 0., 0.).is_none());
        assert!(fit(&a, &b, &s, &s, 32., true, 0., -1.).is_some());
        assert!(fit(&a, &b, &s, &s, 32., true, 2., 1.).is_some());
        assert!(fit(&a, &b, &s, &s, 32., false, 0., -1.).is_none());
    }

    #[test]
    fn the_bed_must_keep_the_groove_and_direction_the_voice_was_sung_to() {
        let (a, b, s) = pair();
        assert!(fit(&a, &b, &s, &s, 32., true, 0., 0.).is_some_and(|f| f > 0.8));
        // A voice from a groove laid over a beatless bed, or the reverse.
        let mut beatless = b.clone();
        for bar in &mut beatless.bars {
            bar.kick_salience = 0.1;
        }
        assert!(fit(&a, &beatless, &s, &s, 32., true, 0., 0.).is_none());
        // The original builds while the incoming bed falls away.
        let (mut rising, mut falling) = (a.clone(), b.clone());
        for (i, bar) in rising.bars.iter_mut().enumerate() {
            bar.rms = 0.08 + 0.004 * i as f32;
        }
        for (i, bar) in falling.bars.iter_mut().enumerate() {
            bar.rms = 0.33 - 0.004 * i as f32;
        }
        assert!(fit(&rising, &falling, &s, &s, 32., true, 0., 0.).is_none());
        // Without heard notes the key match decides.
        assert!(fit(&a, &b, &s, &s, 32., false, 0., 0.).is_none());
    }

    #[test]
    fn the_exit_follows_the_line_and_the_incoming_track() {
        let (a, b, s) = pair();
        // A line still at full voice when it stops is thrown into an echo.
        assert_eq!(shape(&a, &b, &s, &s, 32., true).exit, Exit::Throw);
        // Without a shared beat the stop dissolves into a room instead.
        assert_eq!(shape(&a, &b, &s, &s, 32., false).exit, Exit::Wash);
        // A line that decays is followed down.
        let mut sung_out = a.clone();
        for f in &mut sung_out.stems.as_mut().unwrap().frames {
            if f.start_sec >= 82. {
                f.rms[0] = 0.1 * (84. - f.start_sec).max(0.) / 2. * 0.4;
            }
        }
        assert_eq!(shape(&sung_out, &b, &s, &s, 32., true).exit, Exit::Tail);
        // An incoming drop on the release: thin the voice out of its way.
        let mut dropping = b.clone();
        dropping.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: 84.,
                label: SectionLabel::BuildUp,
            },
            Section {
                start_sec: 84.,
                end_sec: dropping.duration_sec,
                label: SectionLabel::Drop,
            },
        ];
        assert_eq!(shape(&a, &dropping, &s, &s, 32., true).exit, Exit::Sweep);
        // A sparse incoming bed at the release: a long reverb wash.
        let mut sparse = b.clone();
        for bar in sparse.bars.iter_mut().filter(|bar| bar.start_sec >= 78.) {
            bar.kick_salience = 0.1;
        }
        assert_eq!(
            shape(&sung_out, &sparse, &s, &s, 32., true).exit,
            Exit::Wash
        );
        // The voice sits back over a quieter bed, never louder than it was.
        // Quieter than its own usual level while the voice rides: a quieter
        // master alone is not a quieter bed.
        let mut quiet = b.clone();
        for bar in quiet.bars.iter_mut().filter(|bar| bar.start_sec < 84.) {
            bar.rms *= 0.75;
        }
        let quiet = Shape {
            ..shape(&a, &quiet, &s, &s, 32., true)
        };
        assert!(
            quiet.sit_db < -1. && quiet.sit_db >= -3.2,
            "{}",
            quiet.sit_db
        );
        assert!(shape(&a, &b, &s, &s, 32., true).sit_db.abs() < 0.01);
    }

    #[test]
    fn a_carry_brings_the_incoming_parts_in_one_at_a_time_without_a_jump() {
        let n = 16.;
        for exit in [Exit::Tail, Exit::Throw, Exit::Sweep, Exit::Wash] {
            let mut plan = MixPlan {
                summary: Some(MixPlanSummary {
                    pair: (TrackId(1), TrackId(2)),
                    strategy: StrategyId::VocalRide,
                    score: 0.8,
                    used_fallback: false,
                    length_bars: n as u16,
                }),
                clock: Polyline {
                    nodes: vec![(0., 0.), (n, 32.)],
                },
                ..Default::default()
            };
            let shape = Shape {
                leave: 8.,
                voice_end: 15.5,
                exit,
                sit_db: -2.,
                entry_db: 6.,
            };
            crate::stem_mix::carry(&mut plan, &shape, 120.);
            let stems = plan.stem_mix.as_ref().unwrap();
            for stem in 0..4 {
                assert!(
                    stems.incoming[stem].sample(0.) < 0.01,
                    "{exit:?} stem {stem} slams in"
                );
                assert!(
                    stems.incoming[stem].sample(n) > 0.99,
                    "{exit:?} stem {stem} late in"
                );
            }
            assert!(
                stems.outgoing[0].sample(n - 0.25) < 0.01,
                "{exit:?} voice still up"
            );
            assert!(stems.outgoing[0].sample(10.) < 0.81 && stems.outgoing[0].sample(10.) > 0.78);
            let (mut prev_in, mut prev_out) = ([0f32; 4], [1f32; 4]);
            for i in 0..=(n * 16.) as usize {
                let u = i as f32 / 16.;
                let (o, c) = (
                    std::array::from_fn::<f32, 4, _>(|k| stems.outgoing[k].sample(u)),
                    std::array::from_fn::<f32, 4, _>(|k| stems.incoming[k].sample(u)),
                );
                // One bass at a time; tonal layers cross at equal power.
                assert!(o[3] + c[3] <= 1.01, "{exit:?} doubled bass at {u}");
                if u <= 8. {
                    assert!((o[2].powi(2) + c[2].powi(2) - 1.).abs() < 0.02);
                }
                // Drums and tonal layers never jump: no step above a third
                // of full level in a sixteenth of a bar.
                for k in [1, 2] {
                    assert!(
                        (c[k] - prev_in[k]).abs() < 0.34,
                        "{exit:?} stem {k} jumps in at {u}"
                    );
                    assert!(
                        (o[k] - prev_out[k]).abs() < 0.34,
                        "{exit:?} stem {k} jumps out at {u}"
                    );
                }
                (prev_in, prev_out) = (c, o);
            }
            assert!(plan.send_fx.is_some_and(|fx| fx.valid()));
            assert!(
                plan.stages
                    .windows(2)
                    .all(|s| s[1].start_bar >= s[0].end_bar - 0.001)
            );
            assert_eq!(plan.stages.last().map(|s| s.end_bar), Some(n));
        }
    }
}
