use super::*;
use crate::level::{bands, filter_gain, lin, parts, CENTRES};
use mixless_protocol::{Polyline, SectionLabel, StemMix};

// Original route arithmetic, before factor sharing or sample memoization.
fn reference(
    plan: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    u: f32,
    mask: [f32; 4],
) -> [f32; 2] {
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
        let selected = stem.map_or([1.; 4], |s| parts(s, u));
        let p = bands(
            t,
            source.sample(u),
            Some(std::array::from_fn(|i| selected[i] * mask[i])),
        );
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
    [a_bands.iter().sum(), b_bands.iter().sum()]
}

fn track() -> TrackAnalysis {
    crate::stem_regressions::with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.8,
        0.2,
    ))
}
fn plan() -> MixPlan {
    let mut p = MixPlan {
        incoming_start_bar: 0.75,
        outgoing_source: Polyline {
            nodes: vec![(0., 2.), (4., 12.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (0.75, 0.), (4., 7.)],
        },
        ..Default::default()
    };
    p.lanes.xfader.nodes = vec![(0., -1.), (1., -0.2), (2., 0.4), (4., 1.)];
    for eq in [&mut p.lanes.eq_a, &mut p.lanes.eq_b] {
        eq.low.nodes = vec![(0., -3.), (4., 0.)];
        eq.mid.nodes = vec![(0., -12.), (4., -1.)];
        eq.high.nodes = vec![(0., 1.), (4., -4.)];
    }
    for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
        filter.lp_hz.nodes = vec![(0., 20000.), (4., 2200.)];
        filter.hp_hz.nodes = vec![(0., 20.), (4., 400.)];
    }
    p.stem_mix = Some(StemMix {
        outgoing: std::array::from_fn(|s| Polyline {
            nodes: vec![(0., 1.), (4., s as f32 / 4.)],
        }),
        incoming: std::array::from_fn(|s| Polyline {
            nodes: vec![(0., 0.), (4., 1. - s as f32 / 4.)],
        }),
    });
    p
}

#[test]
fn routed_checks_match_original_bits_with_filters_launch_gaps_and_missing_stems() {
    for stems in [true, false] {
        let mut a = track();
        let mut b = track();
        for (i, f) in b.stems.as_mut().unwrap().frames.iter_mut().enumerate() {
            f.band_db = std::array::from_fn(|s| {
                std::array::from_fn(|band| -80. + ((i * (s + 1) + band * 3) % 70) as f32)
            });
        }
        b.stems
            .as_mut()
            .unwrap()
            .frames
            .retain(|f| !(0.5..0.9).contains(&f.start_sec));
        if !stems {
            a.stems = None;
            b.stems = None;
        }
        let mut p = plan();
        for variant in 0..3 {
            if variant == 1 {
                p.stem_mix = None;
            }
            if variant == 2 {
                p.outgoing_source.nodes.clear();
                p.incoming_start_bar = f32::NAN;
            }
            let mut times: Vec<_> = (0..65).map(|i| i as f32 / 16.).collect();
            times.extend([-0., 0.75f32.next_down(), 0.75f32.next_up(), f32::NAN]);
            let verify = || {
                for &u in &times {
                    assert_eq!(
                        crate::level::deck_power(&p, &a, &b, u).map(f32::to_bits),
                        reference(&p, &a, &b, u, [1.; 4]).map(f32::to_bits)
                    );
                    if let Some((raw, heard)) = crate::level::foreground_power(&p, &a, &b, u) {
                        let mask = [1., 0., 1., 0.];
                        let expected = [
                            bands(&a, p.outgoing_source.sample(u), Some(mask))
                                .iter()
                                .sum::<f32>(),
                            bands(&b, p.incoming_source.sample(u), Some(mask))
                                .iter()
                                .sum::<f32>(),
                        ];
                        assert_eq!(raw.map(f32::to_bits), expected.map(f32::to_bits));
                        assert_eq!(
                            heard.map(f32::to_bits),
                            reference(&p, &a, &b, u, mask).map(f32::to_bits)
                        );
                    } else {
                        assert!(!stems);
                    }
                }
            };
            verify();
            with_plan(&p, &a, &b, || {
                verify();
                verify();
            });
        }
    }
}

#[test]
fn exact_keys_foreign_plans_nested_scopes_and_edits_are_isolated() {
    let a = track();
    let b = track();
    let mut p = plan();
    let foreign = p.clone();
    with_plan(&p, &a, &b, || {
        assert_eq!(full(&p, &a, &b, 1., || [1., 2.]), [1., 2.]);
        assert_eq!(
            full(&p, &a, &b, 1., || panic!("lost cached result")),
            [1., 2.]
        );
        assert_eq!(full(&p, &a, &b, 1f32.next_up(), || [3., 4.]), [3., 4.]);
        assert_eq!(full(&foreign, &a, &b, 1., || [5., 6.]), [5., 6.]);
        assert_eq!(full(&foreign, &a, &b, 1., || [7., 8.]), [7., 8.]);
        assert_eq!(full(&p, &b, &a, 1., || [9., 10.]), [9., 10.]);
        with_plan(&foreign, &a, &b, || {
            assert_eq!(full(&foreign, &a, &b, 1., || [11., 12.]), [11., 12.]);
        });
        assert_eq!(
            full(&p, &a, &b, 1., || panic!("nested scope lost parent")),
            [1., 2.]
        );
    });
    p.lanes.gain_a = Polyline::constant(-6.);
    with_plan(&p, &a, &b, || {
        assert_eq!(full(&p, &a, &b, 1., || [13., 14.]), [13., 14.]);
    });
    assert!(CURRENT.with(|c| c.borrow().is_none()));
}
