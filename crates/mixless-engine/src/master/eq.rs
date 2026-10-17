//! RBJ peaking filters: https://www.w3.org/TR/audio-eq-cookbook/
use crate::dsp::SmoothValue;
use mixless_protocol::MasterEqBand;

#[derive(Clone, Copy)]
struct Coefficients {
    b: [f64; 3],
    a: [f64; 2],
}

impl Coefficients {
    fn new(band: MasterEqBand, sr: f32) -> Self {
        let w = std::f64::consts::TAU * f64::from(band.frequency_hz.min(sr * 0.45) / sr);
        let a = 10_f64.powf(f64::from(band.gain_db) / 40.);
        let alpha = w.sin() / (2. * f64::from(band.q));
        let a0 = 1. + alpha / a;
        Self {
            b: [
                (1. + alpha * a) / a0,
                -2. * w.cos() / a0,
                (1. - alpha * a) / a0,
            ],
            a: [-2. * w.cos() / a0, (1. - alpha / a) / a0],
        }
    }
    fn response(self, hz: f32, sr: f32) -> f32 {
        let w = std::f64::consts::TAU * f64::from(hz / sr);
        let norm = |c: [f64; 3]| {
            let re = c[0] + c[1] * w.cos() + c[2] * (2. * w).cos();
            let im = -c[1] * w.sin() - c[2] * (2. * w).sin();
            re * re + im * im
        };
        (10. * (norm(self.b) / norm([1., self.a[0], self.a[1]]))
            .max(1e-20)
            .log10()) as f32
    }
}

/// The editor uses the actual digital filter response, including Nyquist limits.
pub fn master_eq_response(bands: &[MasterEqBand; 5], hz: f32, sr: f32) -> f32 {
    bands
        .iter()
        .map(|b| Coefficients::new(*b, sr).response(hz, sr))
        .sum()
}

pub(super) struct EqBand {
    frequency: SmoothValue,
    gain: SmoothValue,
    q: SmoothValue,
    last: MasterEqBand,
    coefficients: Coefficients,
    state: [[f64; 2]; 2],
}

impl EqBand {
    pub fn new(b: MasterEqBand, sr: f32) -> Self {
        Self {
            frequency: SmoothValue::new(b.frequency_hz, sr, 0.02),
            gain: SmoothValue::new(b.gain_db, sr, 0.02),
            q: SmoothValue::new(b.q, sr, 0.02),
            last: b,
            coefficients: Coefficients::new(b, sr),
            state: [[0.; 2]; 2],
        }
    }
    pub fn configure(&mut self, b: MasterEqBand) {
        self.frequency.set(b.frequency_hz);
        self.gain.set(b.gain_db);
        self.q.set(b.q);
    }
    pub fn process(&mut self, input: [f32; 2], sr: f32, refresh: bool) -> [f32; 2] {
        let band = MasterEqBand {
            frequency_hz: self.frequency.next(),
            gain_db: self.gain.next(),
            q: self.q.next(),
        };
        if refresh && band != self.last {
            self.last = band;
            self.coefficients = Coefficients::new(band, sr);
        }
        let c = self.coefficients;
        std::array::from_fn(|i| {
            let x = f64::from(input[i]);
            let y = c.b[0] * x + self.state[i][0];
            self.state[i][0] = tiny(c.b[1] * x - c.a[0] * y + self.state[i][1]);
            self.state[i][1] = tiny(c.b[2] * x - c.a[1] * y);
            y as f32
        })
    }
}
fn tiny(v: f64) -> f64 {
    if v.abs() < 1e-20 { 0. } else { v }
}
