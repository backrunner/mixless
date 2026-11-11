//! Protect heard vocal phrases, independently of instrumental tonal foreground.
use mixless_protocol::{BarFeature, TrackAnalysis};

mod evidence;
pub(crate) mod pattern;
pub(crate) use evidence::{continuation, held_note, note_supported, Continuation, Scope};

pub(crate) fn continues(t: &TrackAnalysis, sec: f32) -> bool {
    continuation(t, sec) == Continuation::Continuing
}

#[cfg(test)]
mod phrase_tests {
    use super::*;
    use mixless_protocol::SectionLabel;

    #[test]
    fn interword_rests_do_not_release_a_line_across_gain_grid_or_source_position() {
        for bpm in [60., 120., 240.] {
            for gain in [0.01, 1., 4.] {
                for shift in [0., 12.] {
                    let mut t = crate::stem_regressions::with_stems(crate::tests::track(
                        1,
                        bpm,
                        "8A",
                        SectionLabel::Outro,
                        64,
                        0.8,
                        0.3,
                    ));
                    for f in &mut t.stems.as_mut().unwrap().frames {
                        let at = f.start_sec - shift;
                        f.vocal_activity = if (2.0..3.6).contains(&at) || (4.4..7.0).contains(&at) {
                            1.
                        } else {
                            0.
                        };
                        for rms in &mut f.rms {
                            *rms *= gain;
                        }
                    }
                    assert!(continues(&t, shift + 4.), "{bpm} {gain} {shift}");
                    assert!(!safe_entry(&t, shift + 4.));
                    assert!(!crate::continuity::voice_released(&t, shift + 4.));
                    assert!(!continues(&t, shift + 7.2));
                    assert!(crate::continuity::voice_released(&t, shift + 7.2));
                    // A full phrase can begin after a longer instrumental rest.
                    for f in &mut t.stems.as_mut().unwrap().frames {
                        if f.start_sec >= shift + 3.2 && f.start_sec < shift + 4.4 {
                            f.vocal_activity = 0.;
                        }
                    }
                    assert!(!continues(&t, shift + 4.4));
                }
            }
        }
    }
}

pub(crate) fn risk(bar: &BarFeature) -> f32 {
    bar.vocal_confidence.unwrap_or(bar.vocal_presence)
}

/// Only repeated, articulated local motifs may behave as a rhythmic layer.
/// Dense short notes alone do not distinguish chops from a sung sentence.
pub(crate) fn chops(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    pattern::window(t, start, end)
}

pub(crate) fn phrases(t: &TrackAnalysis) -> Vec<(f32, f32)> {
    let gap = 240. / t.tempo.global_bpm.max(30.);
    let mut result: Vec<(f32, f32)> = vec![];
    for bar in t.bars.iter().filter(|b| b.rms > 0.001 && risk(b) >= 0.45) {
        if let Some(last) = result.last_mut().filter(|last| {
            bar.start_sec - last.1 <= gap + 0.01
                || (bar.start_sec - last.1 <= 2. * gap + 0.01
                    && t.bars
                        .iter()
                        .filter(|b| {
                            b.start_sec >= last.1 - 0.01 && b.end_sec <= bar.start_sec + 0.01
                        })
                        .all(|b| risk(b) >= 0.25))
        }) {
            last.1 = bar.end_sec;
        } else {
            result.push((bar.start_sec, bar.end_sec));
        }
    }
    result
}

pub(crate) fn safe_exit(t: &TrackAnalysis, end: f32) -> bool {
    crate::continuity::voice_cut_safe(t, end)
}

pub(crate) fn safe_entry(t: &TrackAnalysis, start: f32) -> bool {
    if continues(t, start) {
        return false;
    }
    if t.stems
        .as_ref()
        .is_some_and(|s| s.note_crossing(start, mixless_protocol::StemKind::Vocals))
        && crate::continuity::voice_active(t, start) > 0.2
    {
        return false;
    }
    // Starting a phrase is fine; a cue inside an already sung syllable is not.
    if crate::phrasing::boundary_quality(t, start) >= 0.6 {
        return true;
    }
    crate::continuity::voice_cut_safe(t, start)
}

pub(crate) fn last_end(t: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    phrases(t)
        .into_iter()
        .filter(|&(a, b)| a < end - 0.05 && b > start)
        .map(|(_, b)| b)
        .max_by(f32::total_cmp)
}

/// Keep one singer in front and propose a level/EQ release. The final audible
/// foreground check still requires that a sung word survives the release.
pub(crate) fn protect(plan: &mut mixless_protocol::MixPlan, t: &TrackAnalysis) -> Option<()> {
    if last_end(t, plan.t_in_a, plan.t_out_a).is_none() {
        return Some(());
    }
    let n = plan.summary.as_ref()?.length_bars as f32;
    if n < 4. {
        return None;
    }
    let g = crate::grid::Grid(t);
    let end = if t.moments.is_empty() {
        let end = last_end(t, plan.t_in_a, plan.t_out_a)?;
        if end
            > plan.t_out_a
                - (240. / t.tempo.global_bpm.max(30.)).min((plan.t_out_a - plan.t_in_a) * 0.25)
                + 0.05
        {
            return None;
        }
        end
    } else {
        crate::continuity::fade_start(t, plan.t_in_a, plan.t_out_a)
    };
    let hold = ((g.beat(end) - g.beat(plan.t_in_a)) / g.meter()).clamp(0., n - 1.);
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    let bass = plan.handoff_bar.unwrap_or(n * 0.55);
    let to_db = |gain: f32| (20. * gain.max(0.000016).log10()).clamp(-96., 6.);
    let lanes = &mut plan.lanes;
    for lane in [
        &mut lanes.xfader,
        &mut lanes.gain_a,
        &mut lanes.eq_a.mid,
        &mut lanes.eq_a.high,
        &mut lanes.filter_a.lp_hz,
        &mut lanes.filter_a.hp_hz,
        &mut lanes.fx_send_a,
        &mut lanes.eq_a.low,
        &mut lanes.eq_b.low,
        &mut lanes.eq_b.mid,
    ] {
        lane.nodes.clear();
    }
    for i in 0..=n as usize * 64 {
        let u = i as f32 / 64.;
        let p = if u <= hold {
            ease(u / hold.max(0.01)) / 3.
        } else {
            1. / 3. + 2. / 3. * ease((u - hold) / (n - hold))
        };
        let tail = ease((u - hold) / (n - hold));
        let fader_a = (1. - p).sqrt().max(0.00002);
        let gain = if u <= hold {
            1. / fader_a
        } else {
            (1. - tail) / fader_a
        };
        lanes.xfader.nodes.push((u, 2. * p - 1.));
        lanes.gain_a.nodes.push((
            u,
            if u >= n {
                -96.
            } else {
                (20. * gain.max(0.00002).log10()).clamp(-96., 6.)
            },
        ));
        lanes.eq_a.mid.nodes.push((u, -12. * tail));
        lanes.eq_a.high.nodes.push((u, -3. * tail));
        lanes.filter_a.lp_hz.nodes.push((u, 20000.));
        lanes.filter_a.hp_hz.nodes.push((u, 20.));
        lanes.fx_send_a.nodes.push((u, 0.));
        lanes
            .eq_b
            .mid
            .nodes
            .push((u, -18. * (1. - ease((tail - 0.6) / 0.4))));
        // Recompute bass compensation against the new vocal-preserving levels.
        // Reusing the old EQ curve would boost both bass lines during the hold.
        let exchange = ease((u - bass + 0.125) / 0.25) * std::f32::consts::FRAC_PI_2;
        lanes.eq_a.low.nodes.push((
            u,
            to_db(exchange.cos().max(0.) / (fader_a * gain).max(0.00002)),
        ));
        let level_b = p.sqrt() * 10f32.powf(lanes.gain_b.sample(u) / 20.);
        lanes
            .eq_b
            .low
            .nodes
            .push((u, to_db(exchange.sin().max(0.) / level_b.max(0.00002))));
    }
    plan.stages = vec![
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: hold,
            label: "Layer rhythm and hold vocal foreground".into(),
        },
        mixless_protocol::MixStage {
            start_bar: hold,
            end_bar: n,
            label: "Vocal gap / soft vocal exit".into(),
        },
    ];
    plan.stages.retain(|s| s.end_bar > s.start_bar);
    Some(())
}

#[cfg(test)]
pub(crate) mod chop_tests {
    use super::*;
    use mixless_protocol::{
        SectionLabel as S, StemAnalysis, StemFrame, StemKind, StemNote, TrackId,
    };

    pub(crate) fn track(vocal: bool) -> TrackAnalysis {
        let mut t = crate::tests::track(1, 128., "8A", S::Drop, 32, 0.9, 0.3);
        let mut stems = StemAnalysis {
            version: 2,
            separator_sha256: String::new(),
            notes_sha256: String::new(),
            duration_sec: t.duration_sec,
            residual_rms: 0.01,
            frames: (0..(t.duration_sec / 0.025) as usize)
                .map(|i| {
                    let active = ((i as f32 + 0.5) * 0.025).rem_euclid(0.25) < 0.1;
                    StemFrame {
                        start_sec: i as f32 * 0.025,
                        end_sec: (i + 1) as f32 * 0.025,
                        rms: if vocal {
                            [if active { 0.05 } else { 0.001 }, 0.02, 0.02]
                        } else {
                            [0., 0., 0.]
                        },
                        band_db: [[-30.; 3]; 3],
                        onset: [0.; 3],
                        drum_low_onset: 0.,
                        vocal_activity: f32::from(vocal && active) * 0.8,
                        note_chroma: [0.; 12],
                    }
                })
                .collect(),
            notes: vec![],
        };
        // Repeated source samples: the envelope corroborates the short notes.
        t.tempo.global_bpm = 120.;
        t.tempo.beats.clear();
        t.tempo.segments.clear();
        let step = if vocal { 0.25 } else { 2. };
        let length = if vocal { 0.1 } else { 1.6 };
        let mut at = 0.;
        while at < t.duration_sec - 1. {
            stems.notes.push(StemNote {
                stem: StemKind::Vocals,
                start_sec: at,
                end_sec: at + length,
                midi: 60,
                confidence: 0.9,
            });
            at += step;
        }
        t.stems = Some(stems);
        t.track_id = TrackId(1);
        t
    }

    #[test]
    fn repeated_short_notes_are_chops_and_a_held_line_is_not() {
        let chopped = track(true);
        assert!(chops(&chopped, 4., 12.));
        let mut sung = track(true);
        for note in &mut sung.stems.as_mut().unwrap().notes {
            note.end_sec = note.start_sec + 1.6;
        }
        assert!(!chops(&sung, 4., 12.), "a held vocal line is not a chop");
        assert!(!chops(&track(false), 4., 12.));
        assert!(!chops(&chopped, 4., 4.02));
    }

    #[test]
    fn a_chop_rest_releases_the_pattern_but_not_the_middle_of_a_slice() {
        let chopped = track(true);
        assert!(pattern::at(&chopped, 8.18).is_some());
        assert!(safe_entry(&chopped, 8.18));
        assert!(safe_exit(&chopped, 8.18));
        assert!(crate::continuity::voice_released(&chopped, 8.18));
        assert!(!safe_entry(&chopped, 8.05));
        assert!(!safe_exit(&chopped, 8.05));
        assert!(!crate::continuity::voice_released(&chopped, 8.05));
        let mut sung = chopped.clone();
        // A vocal line over a repeated backing retains its protection.
        sung.stems.as_mut().unwrap().notes.push(StemNote {
            stem: StemKind::Vocals,
            start_sec: 7.,
            end_sec: 10.,
            midi: 67,
            confidence: 0.9,
        });
        sung.stems
            .as_mut()
            .unwrap()
            .notes
            .sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
        assert!(!safe_exit(&sung, 8.18));
    }

    #[test]
    fn pattern_cache_does_not_survive_edits_or_nested_planning() {
        let mut a = track(true);
        let b = track(false);
        {
            let _outer = pattern::Scope::new(&a, &b);
            assert!(pattern::at(&a, 8.).is_some());
            {
                let _inner = pattern::Scope::new(&b, &b);
                assert!(pattern::at(&b, 8.).is_none());
            }
            assert!(pattern::at(&a, 8.).is_some());
        }
        a.stems.as_mut().unwrap().notes.clear();
        let _next = pattern::Scope::new(&a, &b);
        assert!(pattern::at(&a, 8.).is_none());
    }
}
