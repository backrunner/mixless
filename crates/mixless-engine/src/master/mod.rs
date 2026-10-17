//! Allocation-free, stereo-linked master EQ, compressor, expander/gate and limiter.
mod eq;
pub use eq::master_eq_response;
#[cfg(test)]
mod tests;
use crate::dsp::SmoothValue;
use mixless_protocol::{MasterChainMeters, MasterChainSettings, MasterStageMeter};

pub(crate) struct MasterChain {
    sr: f32,
    settings: MasterChainSettings,
    eq: [eq::EqBand; 5],
    eq_mix: SmoothValue,
    compressor_mix: SmoothValue,
    dynamics_mix: SmoothValue,
    makeup: SmoothValue,
    compressor_gain: f32,
    dynamics_gain: f32,
    limiter_gain: f32,
    ceiling: SmoothValue,
    ceiling_target: f32,
    compressor_times: [f32; 2],
    dynamics_times: [f32; 2],
    limiter_release: f32,
    hold_frames: u32,
    gate_open: bool,
    counter: u32,
    pub meters: MasterChainMeters,
    energy: [f64; 2],
    frames: usize,
}

fn coefficient(sr: f32, ms: f32) -> f32 {
    1. - (-1.0_f64 / (f64::from(sr) * f64::from(ms) / 1000.)).exp() as f32
}
pub(super) fn db(v: f32) -> f32 {
    20. * v.max(1e-6).log10()
}
fn linear(db: f32) -> f32 {
    10_f32.powf(db / 20.)
}
fn peak(v: [f32; 2]) -> f32 {
    v[0].abs().max(v[1].abs())
}
fn blend(a: [f32; 2], b: [f32; 2], wet: f32) -> [f32; 2] {
    std::array::from_fn(|i| a[i] + wet * (b[i] - a[i]))
}
fn observe(m: &mut MasterStageMeter, input: [f32; 2], output: [f32; 2], reduction: f32) {
    for i in 0..2 {
        m.input[i] = m.input[i].max(input[i].abs());
        m.output[i] = m.output[i].max(output[i].abs());
    }
    m.reduction_db = m.reduction_db.max(reduction);
}

impl MasterChain {
    pub fn new(sr: f32) -> Self {
        let p = MasterChainSettings::default();
        Self {
            sr,
            settings: p,
            eq: p.eq.map(|b| eq::EqBand::new(b, sr)),
            eq_mix: SmoothValue::new(0., sr, 0.01),
            compressor_mix: SmoothValue::new(0., sr, 0.01),
            dynamics_mix: SmoothValue::new(0., sr, 0.01),
            makeup: SmoothValue::new(1., sr, 0.01),
            compressor_gain: 1.,
            dynamics_gain: 1.,
            limiter_gain: 1.,
            ceiling: SmoothValue::new(linear(p.limiter.ceiling_db), sr, 0.01),
            ceiling_target: linear(p.limiter.ceiling_db),
            compressor_times: [
                coefficient(sr, p.compressor.attack_ms),
                coefficient(sr, p.compressor.release_ms),
            ],
            dynamics_times: [
                coefficient(sr, p.dynamics.attack_ms),
                coefficient(sr, p.dynamics.release_ms),
            ],
            limiter_release: coefficient(sr, p.limiter.release_ms),
            hold_frames: 0,
            gate_open: false,
            counter: 0,
            meters: MasterChainMeters::default(),
            energy: [0.; 2],
            frames: 0,
        }
    }

    pub fn configure(&mut self, p: MasterChainSettings) {
        if p == self.settings {
            return;
        }
        debug_assert!(p.is_valid());
        self.settings = p;
        for (b, settings) in self.eq.iter_mut().zip(p.eq) {
            b.configure(settings);
        }
        self.eq_mix.set(f32::from(p.eq_enabled));
        self.compressor_mix.set(f32::from(p.compressor.enabled));
        self.dynamics_mix.set(f32::from(p.dynamics.enabled));
        self.makeup.set(linear(p.compressor.makeup_db));
        self.ceiling_target = linear(p.limiter.ceiling_db);
        self.ceiling.set(self.ceiling_target);
        self.compressor_times = [
            coefficient(self.sr, p.compressor.attack_ms),
            coefficient(self.sr, p.compressor.release_ms),
        ];
        self.dynamics_times = [
            coefficient(self.sr, p.dynamics.attack_ms),
            coefficient(self.sr, p.dynamics.release_ms),
        ];
        self.limiter_release = coefficient(self.sr, p.limiter.release_ms);
    }

    pub fn begin_block(&mut self) {
        self.meters = MasterChainMeters::default();
        self.energy = [0.; 2];
        self.frames = 0;
    }
    pub fn finish_block(&mut self) {
        self.meters.rms = self
            .energy
            .map(|v| (v / self.frames.max(1) as f64).sqrt() as f32);
    }

    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        // A non-finite upstream sample must not poison persistent filter state.
        let input = input.map(|v| if v.is_finite() { v } else { 0. });
        let mut equalized = input;
        for band in &mut self.eq {
            equalized = band.process(equalized, self.sr, self.counter == 0);
        }
        self.counter = (self.counter + 1) % 32;
        let signal = blend(input, equalized, self.eq_mix.next());

        let c = self.settings.compressor;
        let wet = self.compressor_mix.next();
        let makeup = self.makeup.next();
        let mut compressed = signal;
        if wet > 0. {
            let target = linear(-c.reduction_db(db(peak(signal))));
            let speed = self.compressor_times[usize::from(target >= self.compressor_gain)];
            self.compressor_gain += speed * (target - self.compressor_gain);
            compressed = blend(
                signal,
                signal.map(|v| v * self.compressor_gain * makeup),
                wet,
            );
        } else {
            self.compressor_gain = 1.;
        }
        observe(
            &mut self.meters.compressor,
            signal,
            compressed,
            -db(self.compressor_gain) * wet,
        );

        let d = self.settings.dynamics;
        let wet = self.dynamics_mix.next();
        let mut expanded = compressed;
        if wet > 0. {
            let level = db(peak(compressed));
            // 3 dB closing hysteresis and hold prevent a gate chattering near its threshold.
            let open = level >= d.threshold_db
                || (d.gate && self.gate_open && level >= d.threshold_db - 3.);
            let reduction = if open {
                self.hold_frames = (self.sr * d.hold_ms * 0.001) as u32;
                self.gate_open = true;
                0.
            } else if self.hold_frames > 0 {
                self.hold_frames -= 1;
                0.
            } else {
                self.gate_open = false;
                d.reduction_db(level)
            };
            let target = linear(-reduction);
            let speed = self.dynamics_times[usize::from(target < self.dynamics_gain)];
            self.dynamics_gain += speed * (target - self.dynamics_gain);
            expanded = blend(compressed, compressed.map(|v| v * self.dynamics_gain), wet);
        } else {
            self.dynamics_gain = 1.;
            self.hold_frames = 0;
            self.gate_open = false;
        }
        observe(
            &mut self.meters.dynamics,
            compressed,
            expanded,
            -db(self.dynamics_gain) * wet,
        );

        // Immediate stereo-linked sample-peak protection, no added transport latency.
        // Clamp to the requested ceiling immediately when lowering it; smooth upward.
        let ceiling = self.ceiling.next().min(self.ceiling_target);
        let required = (ceiling / peak(expanded).max(1e-12)).min(1.);
        if required < self.limiter_gain {
            self.limiter_gain = required;
        } else {
            self.limiter_gain += self.limiter_release * (required - self.limiter_gain);
        }
        let output = expanded.map(|v| v * self.limiter_gain);
        observe(
            &mut self.meters.limiter,
            expanded,
            output,
            -db(self.limiter_gain),
        );
        for (energy, v) in self.energy.iter_mut().zip(output) {
            *energy += f64::from(v).powi(2);
        }
        self.frames += 1;
        output
    }
}
