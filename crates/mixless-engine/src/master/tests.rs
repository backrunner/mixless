use super::*;

#[test]
fn default_chain_preserves_existing_limiter_and_stereo_image() {
    let mut chain = MasterChain::new(48000.);
    let mut old = crate::dsp::MasterLimiter::new(48000.);
    for i in 0..48000 {
        let v = (i as f32 * 0.071).sin() * if i < 24000 { 0.4 } else { 3. };
        let a = chain.process([v, -v * 0.5]);
        let b = old.process(v, -v * 0.5);
        assert!(
            (a[0] - b.0).abs() < 1e-6 && (a[1] - b.1).abs() < 1e-6,
            "frame={i} new={a:?} old={b:?}"
        );
        assert!(a[0].abs() <= 0.980001);
        assert!((a[0] * -0.5 - a[1]).abs() < 1e-6);
    }
}

#[test]
fn measured_eq_matches_the_displayed_response_at_all_device_rates() {
    for sr in [8000., 44100., 48000., 96000., 192000.] {
        for gain in [-12., 12.] {
            let mut p = MasterChainSettings {
                eq_enabled: true,
                ..Default::default()
            };
            p.eq[2].gain_db = gain;
            let mut chain = MasterChain::new(sr);
            chain.configure(p);
            let mut energy = [0_f64; 2];
            for i in 0..sr as usize {
                let input = (i as f32 * std::f32::consts::TAU * 1000. / sr).sin() * 0.01;
                let output = chain.process([input, input])[0];
                if i > sr as usize / 2 {
                    energy[0] += f64::from(input).powi(2);
                    energy[1] += f64::from(output).powi(2);
                }
            }
            let measured = (10. * (energy[1] / energy[0]).log10()) as f32;
            let displayed = master_eq_response(&p.eq, 1000., sr);
            assert!(
                (measured - displayed).abs() < 0.02,
                "sr={sr} measured={measured} displayed={displayed}"
            );
        }
    }
}

#[test]
fn compressor_ratio_makeup_and_bypass_are_measured() {
    let mut p = MasterChainSettings::default();
    p.compressor.enabled = true;
    p.compressor.knee_db = 0.;
    p.compressor.threshold_db = -24.;
    p.compressor.ratio = 4.;
    let input = linear(-12.);
    let mut c = MasterChain::new(48000.);
    c.configure(p);
    let mut out = [0.; 2];
    for _ in 0..96000 {
        out = c.process([input, input * 0.5]);
    }
    assert!((db(out[0]) - (-21.)).abs() < 0.02);
    assert!((out[0] * 0.5 - out[1]).abs() < 1e-6);
    p.compressor.makeup_db = 3.;
    c.configure(p);
    for _ in 0..48000 {
        out = c.process([input, input * 0.5]);
    }
    assert!((db(out[0]) - (-18.)).abs() < 0.02);
    p.compressor.enabled = false;
    c.configure(p);
    for _ in 0..48000 {
        out = c.process([input, input * 0.5]);
    }
    assert!((out[0] - input).abs() < 1e-6);
}

#[test]
fn expander_gate_hold_hysteresis_and_bypass() {
    let mut p = MasterChainSettings::default();
    p.dynamics.enabled = true;
    p.dynamics.threshold_db = -30.;
    p.dynamics.hold_ms = 50.;
    p.dynamics.release_ms = 10.;
    p.dynamics.range_db = 40.;
    let mut c = MasterChain::new(48000.);
    c.configure(p);
    let low = linear(-40.);
    let mut out = [0.; 2];
    for _ in 0..48000 {
        out = c.process([low, low]);
    }
    assert!((db(out[0]) + 50.).abs() < 0.03);
    p.dynamics.gate = true;
    c.configure(p);
    for _ in 0..48000 {
        out = c.process([low, low]);
    }
    assert!((db(out[0]) + 80.).abs() < 0.05);
    for _ in 0..48000 {
        c.process([0.1, 0.1]);
    }
    for _ in 0..2000 {
        out = c.process([low, low]);
    }
    assert!((out[0] - low).abs() < 1e-6, "hold must keep gate open");
    for _ in 0..48000 {
        out = c.process([low, low]);
    }
    assert!((db(out[0]) + 80.).abs() < 0.05);
    p.dynamics.enabled = false;
    c.configure(p);
    for _ in 0..48000 {
        out = c.process([low, low]);
    }
    assert!((out[0] - low).abs() < 1e-6);
}

#[test]
fn limiter_ceiling_telemetry_and_release() {
    let mut p = MasterChainSettings::default();
    p.limiter.ceiling_db = -6.;
    let mut c = MasterChain::new(48000.);
    c.configure(p);
    c.begin_block();
    for _ in 0..512 {
        let out = c.process([2., 1.]);
        assert!(out[0] <= linear(-6.) + 1e-6);
        assert!((out[0] * 0.5 - out[1]).abs() < 1e-6);
    }
    c.finish_block();
    assert!((c.meters.limiter.reduction_db - 12.0206).abs() < 0.01);
    assert!((c.meters.rms[0] - linear(-6.)).abs() < 1e-6);
    for _ in 0..48000 {
        c.process([0., 0.]);
    }
    assert!((c.process([0.1, 0.05])[0] - 0.1).abs() < 0.0001);
}

#[test]
fn extreme_live_changes_remain_finite_and_bounded() {
    for sr in [8000., 48000., 192000.] {
        let mut c = MasterChain::new(sr);
        let mut p = MasterChainSettings::default();
        for i in 0..48000 {
            if i % 997 == 0 {
                p.eq_enabled = !p.eq_enabled;
                p.compressor.enabled = !p.compressor.enabled;
                p.dynamics.enabled = !p.dynamics.enabled;
                for b in &mut p.eq {
                    b.frequency_hz = if p.eq_enabled { 20. } else { 20000. };
                    b.gain_db = if p.eq_enabled { 12. } else { -12. };
                    b.q = 10.;
                }
                c.configure(p);
            }
            let input = if i % 511 == 0 {
                f32::NAN
            } else {
                (i as f32 * 0.4).sin() * 4.
            };
            assert!(
                c.process([input, -input])
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= 0.98001)
            );
        }
    }
}
