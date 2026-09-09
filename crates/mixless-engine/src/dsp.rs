//! Preallocated DSP. Safe to call from the audio callback.

const PI: f32 = std::f32::consts::PI;

#[derive(Clone, Copy)]
pub(crate) struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub(crate) fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = flush_small(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush_small(self.b2 * x - self.a2 * y);
        y
    }

    fn bandpass(sr: f32, frequency: f32, quality: f32) -> Self {
        let omega = 2. * PI * (frequency / sr).clamp(0.0001, 0.45);
        let alpha = omega.sin() / (2. * quality);
        let a0 = 1. + alpha;
        Self { b0: alpha / a0, b1: 0., b2: -alpha / a0,
            a1: -2. * omega.cos() / a0, a2: (1. - alpha) / a0, z1: 0., z2: 0. }
    }

    pub(crate) fn lowpass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * (freq / sr).clamp(0.0001, 0.49);
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let b0 = (1.0 - cos) * 0.5;
        let b1 = 1.0 - cos;
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub(crate) fn highpass(sr: f32, freq: f32, q: f32) -> Self {
        let w0 = 2.0 * PI * (freq / sr).clamp(0.0001, 0.49);
        let alpha = w0.sin() / (2.0 * q);
        let cos = w0.cos();
        let b0 = (1.0 + cos) * 0.5;
        let b1 = -(1.0 + cos);
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos;
        let a2 = 1.0 - alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

/// Two cascaded butterworth = LR4.
#[derive(Clone)]
struct Lr4 {
    a: Biquad,
    b: Biquad,
}

impl Lr4 {
    fn lp(sr: f32, freq: f32) -> Self {
        Self {
            a: Biquad::lowpass(sr, freq, std::f32::consts::FRAC_1_SQRT_2),
            b: Biquad::lowpass(sr, freq, std::f32::consts::FRAC_1_SQRT_2),
        }
    }

    fn hp(sr: f32, freq: f32) -> Self {
        Self {
            a: Biquad::highpass(sr, freq, std::f32::consts::FRAC_1_SQRT_2),
            b: Biquad::highpass(sr, freq, std::f32::consts::FRAC_1_SQRT_2),
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        self.b.process(self.a.process(x))
    }
}

/// 3-band isolator + per-band gain/kill.
#[derive(Clone)]
pub struct Isolator {
    low_lp: Lr4,
    split_hp: Lr4,
    low_align_lp: Lr4,
    low_align_hp: Lr4,
    mid_lp: Lr4,
    high_hp: Lr4,
    resonance_low: Biquad,
    resonance_high: Biquad,
    resonance: SmoothValue,
    pub gain: [f32; 3],
    pub kill: [bool; 3],
}

impl Isolator {
    pub fn new(sr: f32) -> Self {
        Self {
            low_lp: Lr4::lp(sr, 150.0),
            split_hp: Lr4::hp(sr, 150.0),
            low_align_lp: Lr4::lp(sr, 2_000.0),
            low_align_hp: Lr4::hp(sr, 2_000.0),
            mid_lp: Lr4::lp(sr, 2_000.0),
            high_hp: Lr4::hp(sr, 2_000.0),
            resonance_low: Biquad::bandpass(sr, 150., 1.8),
            resonance_high: Biquad::bandpass(sr, 2000., 1.8),
            resonance: SmoothValue::new(0.35, sr, 0.012),
            gain: [1.0; 3],
            kill: [false; 3],
        }
    }

    pub fn set_resonance(&mut self, amount: f32) { self.resonance.set(amount.clamp(0., 1.)); }

    pub fn process(&mut self, x: f32) -> f32 {
        let low_split = self.low_lp.process(x);
        let upper_split = self.split_hp.process(x);
        let low = self.low_align_lp.process(low_split) + self.low_align_hp.process(low_split);
        let mid = self.mid_lp.process(upper_split);
        let high = self.high_hp.process(upper_split);
        let g = |i: usize, v: f32| {
            if self.kill[i] {
                0.0
            } else {
                v * self.gain[i]
            }
        };
        // Resonance follows the exposed crossover. Neutral and all-kill remain
        // unchanged; smoothing avoids a click when preferences change mid-track.
        let gains: [f32; 3] = std::array::from_fn(|i| if self.kill[i] { 0. } else { self.gain[i] });
        let low_edge = (gains[0] - gains[1]).abs().min(1.);
        let high_edge = (gains[1] - gains[2]).abs().min(1.);
        let edge = self.resonance_low.process(x) * low_edge + self.resonance_high.process(x) * high_edge;
        g(0, low) + g(1, mid) + g(2, high) + self.resonance.next() * edge
    }
}

/// One-knob filter: <0 LP (keep lows, knob left), >0 HP (keep highs), 0 open.
pub struct ChannelFilter {
    low: StateFilter,
    high: StateFilter,
    sweep: SmoothValue,
    resonance: SmoothValue,
    sample_rate: f32,
    counter: usize,
    wet: f32,
    compensation: f32,
    pub amount: f32,
}

impl ChannelFilter {
    pub fn new(sr: f32) -> Self {
        Self {
            low: StateFilter::new(),
            high: StateFilter::new(),
            sweep: SmoothValue::new(0.0, sr, 0.008),
            resonance: SmoothValue::new(0.35, sr, 0.012),
            sample_rate: sr,
            counter: 0,
            wet: 0.0,
            compensation: 1.0,
            amount: 0.0,
        }
    }

    pub fn set_amount(&mut self, _sr: f32, amount: f32) {
        self.amount = amount.clamp(-1.0, 1.0);
        self.sweep.set(self.amount);
    }

    pub fn set_resonance(&mut self, resonance: f32) {
        self.resonance.set(resonance.clamp(0.0, 1.0));
    }

    pub fn cutoff(sample_rate: f32, amount: f32) -> f32 {
        let upper = 18_000.0f32.min(sample_rate * 0.45);
        if amount <= 0.0 {
            upper * (30.0 / upper).powf(-amount)
        } else {
            30.0 * (upper / 30.0).powf(amount)
        }
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let amount = self.sweep.next();
        let resonance = self.resonance.next();
        if self.counter == 0 {
            let quality = std::f32::consts::FRAC_1_SQRT_2 + 2.8 * resonance;
            self.low.set(
                self.sample_rate,
                Self::cutoff(self.sample_rate, amount.min(0.0)),
                quality,
            );
            self.high.set(
                self.sample_rate,
                Self::cutoff(self.sample_rate, amount.max(0.0)),
                quality,
            );
            self.compensation = (std::f32::consts::FRAC_1_SQRT_2 / quality).powf(0.25);
        }
        self.counter = (self.counter + 1) % 16;
        let low = self.low.process(x).0;
        let high = self.high.process(x).1;
        self.wet = (amount.abs() / 0.08).min(1.0);
        let filtered = if amount < 0.0 { low } else { high };
        x + self.wet * (filtered * self.compensation - x)
    }
}

pub(crate) struct StateFilter {
    first: f32,
    second: f32,
    gain: f32,
    damping: f32,
    norm: f32,
}

impl StateFilter {
    pub(crate) fn new() -> Self {
        Self {
            first: 0.0,
            second: 0.0,
            gain: 0.0,
            damping: 1.0,
            norm: 1.0,
        }
    }

    pub(crate) fn set(&mut self, sample_rate: f32, frequency: f32, quality: f32) {
        self.gain = (PI * (frequency / sample_rate).clamp(0.0001, 0.45)).tan();
        self.damping = 1.0 / quality.max(0.5);
        self.norm = 1.0 / (1.0 + self.gain * (self.gain + self.damping));
    }

    pub(crate) fn process(&mut self, input: f32) -> (f32, f32) {
        let band = (self.first + self.gain * (input - self.second)) * self.norm;
        let low = self.second + self.gain * band;
        self.first = flush_small(2.0 * band - self.first);
        self.second = flush_small(2.0 * low - self.second);
        (low, input - self.damping * band - low)
    }
}

#[derive(Clone)]
pub struct SmoothValue {
    current: f32,
    target: f32,
    increment: f32,
    remaining: u32,
    duration: u32,
}

impl SmoothValue {
    pub fn new(value: f32, sample_rate: f32, seconds: f32) -> Self {
        Self {
            current: value,
            target: value,
            increment: 0.0,
            remaining: 0,
            duration: (sample_rate * seconds).round().max(1.0) as u32,
        }
    }

    pub fn set(&mut self, target: f32) {
        if target.is_finite() && self.target != target {
            self.target = target;
            self.remaining = self.duration;
            self.increment = (target - self.current) / self.duration as f32;
        }
    }

    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current += self.increment;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }
}

pub fn flush_small(value: f32) -> f32 {
    if value.abs() < 1e-20 {
        0.0
    } else {
        value
    }
}

pub fn db_to_lin(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10.0f32.powf(db / 20.0)
    }
}

pub fn xfader_gains(pos: f32, curve: mixless_protocol::XfCurve, reverse: bool) -> (f32, f32) {
    let mut x = pos.clamp(-1.0, 1.0);
    if reverse {
        x = -x;
    }
    let t = (x + 1.0) * 0.5; // 0 = A, 1 = B
    match curve {
        mixless_protocol::XfCurve::Linear => (1.0 - t, t),
        mixless_protocol::XfCurve::EqualPower => {
            let a = (1.0 - t).sqrt();
            let b = t.sqrt();
            (a, b)
        }
        mixless_protocol::XfCurve::Cut => (((1.0 - t) / 0.005).min(1.0), (t / 0.005).min(1.0)),
        mixless_protocol::XfCurve::Scratch => (((1.0 - t) / 0.05).min(1.0), (t / 0.05).min(1.0)),
    }
}

pub struct MasterLimiter {
    gain: f32,
    release: f32,
}

impl MasterLimiter {
    pub fn new(sr: f32) -> Self {
        Self {
            gain: 1.0,
            release: 1.0 - (-1.0 / (sr * 0.08)).exp(),
        }
    }

    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let peak = left.abs().max(right.abs());
        let required = if peak > 0.98 { 0.98 / peak } else { 1.0 };
        if required < self.gain {
            self.gain = required;
        } else {
            self.gain += (required - self.gain) * self.release;
        }
        (left * self.gain, right * self.gain)
    }
}

/// Multi-block equal-power seek crossfade.
pub struct SeekXf {
    remaining: usize,
    total: usize,
}

impl SeekXf {
    pub fn new() -> Self {
        Self {
            remaining: 0,
            total: 0,
        }
    }

    #[allow(dead_code)]
    pub fn start(&mut self, frames: usize) {
        self.remaining = frames.max(1);
        self.total = self.remaining;
    }

    pub fn active(&self) -> bool {
        self.remaining > 0
    }

    pub fn next_gain(&mut self) -> f32 {
        if self.remaining == 0 || self.total == 0 {
            return 1.0;
        }
        let t = 1.0 - self.remaining as f32 / self.total as f32;
        self.remaining -= 1;
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter_rms(sample_rate: f32, amount: f32, resonance: f32, frequency: f32) -> f32 {
        let mut filter = ChannelFilter::new(sample_rate);
        filter.set_amount(sample_rate, amount);
        filter.set_resonance(resonance);
        let mut energy = 0.0;
        for frame in 0..sample_rate as usize {
            let signal = (2.0 * PI * frequency * frame as f32 / sample_rate).sin() * 0.1;
            let output = filter.process(signal);
            if frame >= sample_rate as usize / 2 {
                energy += output * output;
            }
        }
        (energy / (sample_rate * 0.5)).sqrt()
    }

    #[test]
    fn filter_center_is_exact_bypass() {
        let mut filter = ChannelFilter::new(48_000.0);
        for frame in 0..4000 {
            let input = (frame as f32 * 0.17).sin() * 0.6;
            assert_eq!(input, filter.process(input));
        }
    }

    #[test]
    fn filter_endpoints_reject_opposite_band() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
            assert!(filter_rms(sample_rate, -1.0, 0.35, 1000.0) < 0.0002);
            assert!(filter_rms(sample_rate, 1.0, 0.35, 100.0) < 0.00002);
        }
    }

    #[test]
    fn filter_resonance_increases_cutoff_energy() {
        let frequency = ChannelFilter::cutoff(48_000.0, -0.5);
        let low = filter_rms(48_000.0, -0.5, 0.0, frequency);
        let high = filter_rms(48_000.0, -0.5, 1.0, frequency);
        assert!(high > low * 2.0, "resonance: {low} -> {high}");
    }

    #[test]
    fn fast_filter_sweeps_remain_bounded() {
        for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
            let mut filter = ChannelFilter::new(sample_rate);
            filter.set_resonance(1.0);
            let mut last = 0.0f32;
            let mut worst_step = 0.0f32;
            for frame in 0..sample_rate as usize * 2 {
                if frame % 128 == 0 {
                    filter.set_amount(sample_rate, if (frame / 128) % 2 == 0 { -1.0 } else { 1.0 });
                }
                let output =
                    filter.process((frame as f32 * 440.0 * 2.0 * PI / sample_rate).sin() * 0.1);
                assert!(output.is_finite() && output.abs() < 1.0);
                worst_step = worst_step.max((output - last).abs());
                last = output;
            }
            assert!(
                worst_step < 0.10,
                "discontinuity {worst_step} at {sample_rate}"
            );
        }
    }

    #[test]
    fn filter_returns_to_exact_neutral() {
        let mut filter = ChannelFilter::new(48_000.0);
        filter.set_amount(48_000.0, -0.8);
        for _ in 0..2000 {
            filter.process(0.1);
        }
        filter.set_amount(48_000.0, 0.0);
        for _ in 0..2000 {
            filter.process(0.1);
        }
        assert_eq!(filter.process(0.123), 0.123);
    }

    #[test]
    fn scratch_fader_opens_both_channels_in_center() {
        for curve in [
            mixless_protocol::XfCurve::Scratch,
            mixless_protocol::XfCurve::Cut,
        ] {
            assert_eq!(xfader_gains(0.0, curve, false), (1.0, 1.0));
            assert_eq!(xfader_gains(-1.0, curve, false), (1.0, 0.0));
            assert_eq!(xfader_gains(1.0, curve, false), (0.0, 1.0));
            assert_eq!(xfader_gains(-1.0, curve, true), (0.0, 1.0));
        }
    }

    #[test]
    fn limiter_is_transparent_below_ceiling_and_stereo_linked() {
        let mut limiter = MasterLimiter::new(48_000.0);
        assert_eq!(limiter.process(0.8, -0.5), (0.8, -0.5));
        let (left, right) = limiter.process(3.0, 1.5);
        assert!(left <= 0.981 && (left / right - 2.0).abs() < 1e-6);
        for _ in 0..48_000 {
            limiter.process(0.0, 0.0);
        }
        assert!((limiter.process(0.5, 0.5).0 - 0.5).abs() < 0.001);
    }

    #[test]
    fn isolator_unity_and_kill() {
        for frequency in [50.0, 150.0, 500.0, 2000.0, 8000.0] {
            let mut isolator = Isolator::new(48_000.0);
            let mut energy = 0.0;
            for frame in 0..48_000 {
                let input = (std::f32::consts::TAU * frequency * frame as f32 / 48000.0).sin();
                let output = isolator.process(input);
                if frame >= 24000 {
                    energy += output * output;
                }
            }
            assert!(
                (energy / 24000.0 - 0.5).abs() < 0.01,
                "unity at {frequency}: {energy}"
            );
            isolator.kill = [true; 3];
            assert_eq!(isolator.process(0.5), 0.0);
        }
    }

    #[test]
    fn isolated_mid_does_not_boost_crossover() {
        for frequency in [150.0, 2000.0] {
            let mut isolator = Isolator::new(48_000.0);
            isolator.set_resonance(0.);
            isolator.kill = [true, false, true];
            let mut energy = 0.0;
            for frame in 0..48_000 {
                let input = (std::f32::consts::TAU * frequency * frame as f32 / 48000.0).sin();
                let output = isolator.process(input);
                if frame >= 24000 {
                    energy += output * output;
                }
            }
            assert!((energy / 24000.0 - 0.125).abs() < 0.01);
        }
    }
    #[test]
    fn eq_resonance_is_audible_and_neutral_and_all_kill_stay_unchanged() {
        let render = |resonance: f32, gain: [f32; 3]| {
            let mut iso = Isolator::new(48_000.);
            iso.set_resonance(resonance);
            iso.gain = gain;
            let mut output = Vec::new();
            for i in 0..48_000 {
                let sample = iso.process((std::f32::consts::TAU * 150. * i as f32 / 48_000.).sin() * 0.25);
                assert!(sample.is_finite() && sample.abs() < 1.);
                if i >= 24_000 { output.push(sample); }
            }
            output
        };
        let off = render(0., [0.,1.,1.]);
        let on = render(1., [0.,1.,1.]);
        let difference = off.iter().zip(on).map(|(a,b)| (a-b).powi(2)).sum::<f32>() / off.len() as f32;
        assert!(difference > 0.001);
        assert_eq!(render(0., [1.;3]), render(1., [1.;3]));
        assert!(render(1., [0.;3]).iter().all(|v| *v == 0.));
    }

}
