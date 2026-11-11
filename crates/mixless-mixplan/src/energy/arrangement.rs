//! Rank the actual routed exchange, not just its two endpoints or section labels.
use mixless_protocol::{MixPlan, TrackAnalysis};
mod cuts;

/// A quiet destination must not earn a long-blend reward while removing an
/// ongoing foreground. Compare complete local bars against A's natural
/// continuation, allowing its own breaks and fades. This is a soft preference:
/// voice, phrase and harmonic safety still determine which plans are feasible.
pub(crate) fn loss_cost(p: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 2. {
        return cuts::cost(p, a, b);
    }
    if p.lanes.loop_a.is_some() || p.lanes.loop_b.is_some() {
        return 0.;
    }
    let (Some(ra), Some(rb)) = (
        crate::level::reference_power(a),
        crate::level::reference_power(b),
    ) else {
        return 0.;
    };
    // Average a complete local bar so one kick is not a gap. Longer power
    // windows let occasional build impacts conceal several quiet bars.
    let width = 8;
    let mut window = [[0f32; 3]; 8];
    let (mut sum, mut seconds) = (0f32, 0f32);
    let mut foreground = [[0f32; 2]; 8];
    let mut foreground_loss = 0f32;
    let mut foreground_seconds = 0f32;
    for i in 0..(n * 8.) as usize {
        let u = (i as f32 + 0.5) / 8.;
        let raw = 10f32.powf(crate::level::unmixed(a, p.outgoing_source.sample(u)) / 10.) / ra;
        let heard = crate::level::deck_power(p, a, b, u);
        let incoming = 10f32.powf(crate::level::unmixed(b, p.incoming_source.sample(u)) / 10.) / rb;
        window[i % width] = [raw, heard[0] / ra + heard[1] / rb, incoming];
        foreground[i % width] = crate::level::foreground_power(p, a, b, u)
            .map_or([0.; 2], |(raw, heard)| {
                [raw[0] / ra, heard[0] / ra + heard[1] / rb]
            });
        if i + 1 < width {
            continue;
        }
        let expected: f32 = window[..width].iter().map(|v| v[0]).sum::<f32>() / width as f32;
        if expected < 0.01 {
            continue;
        }
        let actual: f32 = window[..width].iter().map(|v| v[1]).sum::<f32>() / width as f32;
        let replacement = window[..width].iter().map(|v| v[2]).sum::<f32>() / width as f32;
        // A full replacement permits the fitter's existing 6 dB exchange
        // budget. A weak destination gets only 3 dB: its quiet material will
        // remain after the control movement ends. Interpolate rather than
        // switching rules at one source-level threshold.
        let budget = 3. + 3. * (replacement / expected).min(1.);
        let loss = (10. * (expected / actual.max(1e-12)).log10() - budget).max(0.);
        let dt = (p.clock.sample(u + 0.0625) - p.clock.sample(u - 0.0625)).max(0.);
        sum += loss * dt;
        // A chopped vocal may carry the entire foreground even when the drums
        // keep total energy high. Price sustained removal against A's natural
        // continuation, not against a still-muted B. A complete local bar
        // ignores tiny rhythmic rests and preserves authored breakdowns.
        let before = foreground.iter().map(|v| v[0]).sum::<f32>() / width as f32;
        let after = foreground.iter().map(|v| v[1]).sum::<f32>() / width as f32;
        if before >= 0.01 {
            let missing = (10. * (before / after.max(1e-12)).log10() - 6.).max(0.);
            foreground_loss += missing * dt;
            if after < before * 0.9 {
                foreground_seconds += dt;
            }
        }
        // A silent pre-roll with A still at unity is not an exchange and
        // cannot dilute a long weak replacement's average loss.
        if heard[0] / ra < raw * 0.9 {
            seconds += dt;
        }
    }
    // Inspect what actually follows the exchange, not B's launch sample.
    // An opening attack can hide a much weaker following phrase. Compare it
    // with A's natural next bar, so an authored breakdown remains available.
    let following = |t: &TrackAnalysis, sec: f32, reference: f32| {
        let g = crate::grid::Grid(t);
        let span = g.meter() * 60. / g.bpm(g.beat(sec));
        (0..8)
            .map(|i| {
                let at = sec + (i as f32 + 0.5) * span / 8.;
                if at >= t.duration_sec {
                    0.
                } else {
                    10f32.powf(crate::level::unmixed(t, at) / 10.) / reference
                }
            })
            .sum::<f32>()
            / 8.
    };
    let expected = following(a, p.outgoing_source.sample(n), ra);
    let actual = following(b, p.incoming_source.sample(n), rb);
    let arrival = if expected >= 0.01 {
        (10. * (expected / actual.max(1e-12)).log10() - 3.).max(0.)
    } else {
        0.
    };
    let gap = crate::experience::continuity(p, a, b).map_or(0., |c| c.gap_cost());
    (0.18
        * ((sum / seconds.max(0.001)).max(foreground_loss / foreground_seconds.max(0.001)) / 6.)
            .min(1.))
    .max(gap)
        + 0.12 * (arrival / 9.).min(1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{MixPlanSummary, Polyline, SectionLabel, StrategyId};

    fn fixture(tempo: f32) -> (TrackAnalysis, TrackAnalysis, MixPlan) {
        let a = crate::tests::track(1, tempo, "8A", SectionLabel::Verse, 64, 0.9, 0.3);
        let mut b = a.clone();
        b.track_id = mixless_protocol::TrackId(2);
        let bar = 240. / tempo;
        let mut p = MixPlan {
            summary: Some(MixPlanSummary {
                pair: (a.track_id, b.track_id),
                strategy: StrategyId::PhraseBlend,
                score: 1.,
                used_fallback: false,
                length_bars: 8,
            }),
            outgoing_source: Polyline {
                nodes: vec![(0., 16. * bar), (8., 24. * bar)],
            },
            incoming_source: Polyline {
                nodes: vec![(0., 0.), (8., 8. * bar)],
            },
            clock: Polyline {
                nodes: vec![(0., 0.), (8., 8. * bar)],
            },
            ..Default::default()
        };
        p.lanes.xfader = Polyline {
            nodes: vec![(0., -1.), (4., 1.), (8., 1.)],
        };
        for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
            filter.lp_hz = Polyline::constant(20000.);
            filter.hp_hz = Polyline::constant(20.);
        }
        (a, b, p)
    }

    #[test]
    fn an_ongoing_verse_cannot_disappear_behind_a_long_nominal_blend() {
        for tempo in [90., 120., 175.] {
            let (a, mut b, p) = fixture(tempo);
            assert_eq!(loss_cost(&p, &a, &b), 0.);
            for bar in b.bars.iter_mut().take(8) {
                bar.rms *= 0.2;
            }
            let expected = loss_cost(&p, &a, &b);
            assert!(expected > 0.15);
            for ga in [0.25f32, 1., 2.] {
                for gb in [0.25f32, 1., 2.] {
                    let (mut a, mut b) = (a.clone(), b.clone());
                    for (t, gain) in [(&mut a, ga), (&mut b, gb)] {
                        for bar in &mut t.bars {
                            bar.rms *= gain;
                            bar.low_db += 20. * gain.log10();
                            bar.mid_db += 20. * gain.log10();
                            bar.high_db += 20. * gain.log10();
                        }
                    }
                    assert!((loss_cost(&p, &a, &b) - expected).abs() < 0.0001);
                }
            }
            let mut released = a.clone();
            for bar in &mut released.bars[16..24] {
                bar.rms *= 0.1;
            }
            assert_eq!(
                loss_cost(&p, &released, &b),
                0.,
                "keep a natural quiet release"
            );
            b.bars.clear();
            assert_eq!(loss_cost(&p, &a, &b), 0., "missing evidence is unknown");
        }
    }

    #[test]
    fn routed_eq_loss_is_visible_even_with_matching_source_windows() {
        let (a, b, mut p) = fixture(120.);
        assert_eq!(loss_cost(&p, &a, &b), 0.);
        for eq in [&mut p.lanes.eq_a, &mut p.lanes.eq_b] {
            eq.low = Polyline::constant(-9.);
            eq.mid = Polyline::constant(-9.);
            eq.high = Polyline::constant(-9.);
        }
        assert!(loss_cost(&p, &a, &b) > 0.);
    }

    #[test]
    fn occasional_build_impacts_cannot_hide_the_quiet_bars_between_them() {
        for tempo in [90., 120., 175.] {
            let (a, mut b, p) = fixture(tempo);
            for (i, bar) in b.bars.iter_mut().take(8).enumerate() {
                bar.rms *= if i % 4 == 0 { 2. } else { 0.2 };
            }
            assert!(loss_cost(&p, &a, &b) > 0.05);
        }
    }

    #[test]
    fn loud_drums_do_not_replace_a_removed_chop_but_a_natural_break_stays_available() {
        for scale in [0.01_f32, 1., 4.] {
            let mut a = crate::vocals::chop_tests::track(true);
            for frame in &mut a.stems.as_mut().unwrap().frames {
                frame.band_db = [[-20.; 3], [-10.; 3], [-90.; 3]];
                for db in frame.band_db.iter_mut().flatten() {
                    *db += 20. * scale.log10();
                }
            }
            let b = a.clone();
            let mut p = fixture(120.).2;
            p.outgoing_source = Polyline {
                nodes: vec![(0., 0.), (8., 16.)],
            };
            p.incoming_source = p.outgoing_source.clone();
            p.lanes.xfader = Polyline::constant(-1.);
            p.stem_mix = Some(mixless_protocol::StemMix {
                outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
                incoming: std::array::from_fn(|_| Polyline::constant(0.)),
            });
            assert_eq!(loss_cost(&p, &a, &b), 0.);
            p.stem_mix.as_mut().unwrap().outgoing[0] = Polyline::constant(0.);
            assert!(loss_cost(&p, &a, &b) > 0.1);
            for frame in &mut a.stems.as_mut().unwrap().frames {
                frame.band_db[0] = [-90. + 20. * scale.log10(); 3];
            }
            assert_eq!(
                loss_cost(&p, &a, &b),
                0.,
                "a muted B does not invent a lost foreground"
            );
        }
    }
}
