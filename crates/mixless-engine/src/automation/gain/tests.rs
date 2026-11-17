use super::*;
use meter::{Metric, Reading};
fn metric(p: f64) -> Metric {
    Metric {
        power: [p; 4],
        short: p,
        peak: 0.2,
        reduction: 0.,
    }
}
fn reading(a: f64, b: f64, sum: f64) -> Reading {
    [metric(a), metric(b), metric(sum), metric(sum), metric(sum)]
}
#[test]
fn joint_gain_respects_crossfader_silence_and_correlation() {
    let full = reading(0.05, 0.05, 0.1);
    assert_eq!(fit::point(&full, &full, [1., 1.], [0.; 2]), [0.; 2]);
    let same_phase = reading(0.05, 0.05, 0.2);
    let cut = fit::point(&same_phase, &full, [1., 1.], [0.; 2]);
    assert!(cut.iter().all(|g| *g < 0.), "{cut:?}");
    let cancelled = reading(0.05, 0.05, 0.001);
    assert_eq!(fit::point(&cancelled, &full, [1., 1.], [0.; 2]), [0.; 2]);
    let missing = reading(0.05, 0.005, 0.055);
    let boost = fit::point(&missing, &full, [1., 1.], [0.; 2]);
    assert_eq!(boost[0], 0.);
    assert!(boost[1] > 1.);
    assert_eq!(fit::point(&missing, &full, [1., 0.], [0.; 2]), [0.; 2]);
    let quiet = reading(0., 1e-10, 1e-10);
    assert_eq!(fit::point(&quiet, &full, [1., 1.], [0.; 2]), [0.; 2]);
}
#[test]
fn retained_high_band_cannot_be_boosted_to_replace_missing_bass() {
    let full = reading(0., 0.1, 0.1);
    let mut shaped = reading(0., 0.025, 0.025);
    for m in &mut shaped[1..] {
        m.power[3] = 0.1;
    }
    let broadband = fit::point(&reading(0., 0.025, 0.025), &full, [0., 1.], [0.; 2]);
    let spectral = fit::point(&shaped, &full, [0., 1.], [0.; 2]);
    assert!(spectral[1] < broadband[1], "{spectral:?} {broadband:?}");
}
#[test]
fn signed_gain_returns_to_neutral_without_crossing_a_gap() {
    let mut v = vec![3.; 51];
    v[25] = 0.;
    v[26..].fill(-3.);
    smooth(&mut v, 0.2, 1.5);
    assert_eq!(v[0], 0.);
    assert_eq!(v[25], 0.);
    assert_eq!(v[50], 0.);
    for w in v.windows(2) {
        assert!((w[1] - w[0]).abs() <= 0.30001);
    }
}

#[test]
fn signed_compensation_shares_slew_across_a_zero_crossing() {
    for sign in [-1., 1.] {
        for offset in [0.1, 1., 3.] {
            let original = [0., 3. * sign, 3. * sign, -offset * sign, -3. * sign, 0.];
            let mut values = original;
            smooth(&mut values, 0.2, 1.5);
            for w in values.windows(2) {
                assert!((w[1] - w[0]).abs() <= 0.30001, "{values:?}");
            }
            for (v, base) in values.into_iter().zip(original) {
                assert!(v * base >= 0. && v.abs() <= base.abs());
            }
        }
    }
}

#[test]
fn validation_rejects_extra_limiter_work_and_inter_sample_peaks() {
    let base = reading(0., 0.025, 0.025);
    let reference = reading(0., 0.1, 0.1);
    let mut improved = reading(0., 0.06, 0.06);
    assert!(fit::acceptable(&[base], &[reference], &[improved]));
    improved[1].reduction = 1.;
    assert!(!fit::acceptable(&[base], &[reference], &[improved]));
    improved[1].reduction = 0.;
    improved[4].peak = 1.1;
    assert!(!fit::acceptable(&[base], &[reference], &[improved]));
}

#[test]
fn output_validation_preserves_natural_transients_without_amplifying_their_contrast() {
    // Total error improves, but boosting the source that supplies the transient
    // adds 6 dB of local output contrast. A slow knob does not prevent this.
    let base = [0.001, 0.005, 0.001].map(|p| reading(0., p, p));
    let reference = [reading(0., 0.1, 0.1); 3];
    let pumping = [0.0015, 0.03, 0.0015].map(|p| reading(0., p, p));
    assert!(!fit::acceptable(&base, &reference, &pumping));
    // A constant level correction leaves the recording's original contrast.
    let retained = [0.002, 0.01, 0.002].map(|p| reading(0., p, p));
    assert!(fit::acceptable(&base, &reference, &retained));
    for band in 1..4 {
        let mut selective = retained;
        selective[1][4].power[band] = 0.03;
        assert!(!fit::acceptable(&base, &reference, &selective));
    }
    let bounded = [0.002, 0.011, 0.002].map(|p| reading(0., p, p));
    assert!(fit::acceptable(&base, &reference, &bounded));
}

#[test]
fn output_validation_checks_between_the_fitting_windows() {
    let base = [metric(0.01); 12];
    for phase in 0..3 {
        let mut shifted = [metric(0.015); 12];
        shifted[4 + phase].power[1] = 0.06;
        // Sampling only at the fitting grid misses this band fluctuation.
        let coarse = [reading(0., 0.015, 0.015); 3];
        assert!(fit::acceptable(
            &[reading(0., 0.01, 0.01); 3],
            &[reading(0., 0.1, 0.1); 3],
            &coarse,
        ));
        assert!(!fit::stable_output_windows(&base, &shifted));
    }
    assert!(fit::stable_output_windows(&base, &[metric(0.015); 12]));
}

#[test]
fn output_validation_checks_band_windows_between_loudness_readouts() {
    let base = render::Output {
        levels: vec![metric(0.01); 12],
        bands: vec![[0.01; 3]; 60],
        step_lag: 20,
    };
    let corrected = render::Output {
        levels: vec![metric(0.015); 12],
        bands: vec![[0.015; 3]; 60],
        step_lag: 20,
    };
    assert!(fit::stable_output(&base, &corrected));
    for band in 0..3 {
        for phase in 1..5 {
            let mut shifted = corrected.clone();
            shifted.bands[20 + phase][band] = 0.06;
            assert!(fit::stable_output_windows(&base.levels, &shifted.levels));
            assert!(!fit::stable_output(&base, &shifted));
        }
    }
    let mut incomplete = corrected.clone();
    incomplete.bands.pop();
    assert!(!fit::stable_output(&base, &incomplete));
}

#[test]
fn gain_decisions_are_stable_under_recording_level_changes() {
    let base = reading(0.05, 0.005, 0.055);
    let reference = reading(0.05, 0.05, 0.1);
    let expected = fit::point(&base, &reference, [1., 1.], [0.; 2]);
    for scale in [0.0625, 0.25, 1., 4.] {
        let transform = |mut r: Reading| {
            for metric in &mut r {
                for power in &mut metric.power {
                    *power *= scale;
                }
                metric.short *= scale;
            }
            r
        };
        assert_eq!(
            fit::point(&transform(base), &transform(reference), [1., 1.], [0.; 2]),
            expected
        );
    }
}
