//! Windowed spectral processing with precomputed FFT tables and overlap-add.

// Fixed 256 point radix-2 FFT with four Hann overlaps. All tables and rings
// are created before playback; the spectral transform uses stack arrays.
const FFT_N: usize = 256;
pub(super) struct Spectral {
    input: [[f32; 2]; FFT_N],
    output: [[f32; 2]; FFT_N],
    window: [f32; FFT_N],
    twiddle: [[f32; 2]; FFT_N / 2],
    index: usize,
    count: usize,
}
impl Spectral {
    pub(super) fn new() -> Self {
        Self {
            input: [[0.0; 2]; FFT_N],
            output: [[0.0; 2]; FFT_N],
            window: std::array::from_fn(|i| {
                0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_N as f32).cos()
            }),
            twiddle: std::array::from_fn(|i| {
                let a = -std::f32::consts::TAU * i as f32 / FFT_N as f32;
                [a.cos(), a.sin()]
            }),
            index: 0,
            count: 0,
        }
    }
    pub(super) fn reset(&mut self) {
        self.input.fill([0.0; 2]);
        self.output.fill([0.0; 2]);
        self.index = 0;
        self.count = 0;
    }
    fn fft(&self, b: &mut [[f32; 2]; FFT_N], inverse: bool) {
        for i in 0..FFT_N {
            let j = i.reverse_bits() >> (usize::BITS - 8);
            if i < j {
                b.swap(i, j);
            }
        }
        let mut span = 2;
        while span <= FFT_N {
            for start in (0..FFT_N).step_by(span) {
                for k in 0..span / 2 {
                    let [wr, mut wi] = self.twiddle[k * FFT_N / span];
                    if inverse {
                        wi = -wi;
                    }
                    let a = b[start + k];
                    let x = b[start + k + span / 2];
                    let z = [x[0] * wr - x[1] * wi, x[0] * wi + x[1] * wr];
                    b[start + k] = [a[0] + z[0], a[1] + z[1]];
                    b[start + k + span / 2] = [a[0] - z[0], a[1] - z[1]];
                }
            }
            span *= 2;
        }
    }
    pub(super) fn process(&mut self, input: [f32; 2], depth: f32) -> [f32; 2] {
        let out = self.output[self.index];
        self.output[self.index] = [0.0; 2];
        self.input[self.index] = input;
        self.index = (self.index + 1) % FFT_N;
        self.count = self.count.wrapping_add(1);
        if self.count % (FFT_N / 4) == 0 {
            for ch in 0..2 {
                let mut bins = std::array::from_fn(|i| {
                    [
                        self.input[(self.index + i) % FFT_N][ch] * self.window[i],
                        0.0,
                    ]
                });
                self.fft(&mut bins, false);
                let stride = 2 + (depth * 10.0) as usize;
                for k in 1..FFT_N / 2 {
                    let gain = if k % stride == 0 { 1.0 } else { 1.0 - depth };
                    let a = depth * (k % 7) as f32 * 0.25;
                    let (s, c) = a.sin_cos();
                    let x = bins[k];
                    bins[k] = [gain * (x[0] * c - x[1] * s), gain * (x[0] * s + x[1] * c)];
                    bins[FFT_N - k] = [bins[k][0], -bins[k][1]];
                }
                self.fft(&mut bins, true);
                for i in 0..FFT_N {
                    self.output[(self.index + i) % FFT_N][ch] +=
                        bins[i][0] * self.window[i] / (FFT_N as f32 * 1.5);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectral_overlap_add_reconstructs_neutral_input() {
        let mut spectral = Spectral::new();
        let input: Vec<_> = (0..4096).map(|i| (i as f32 * 0.13).sin() * 0.2).collect();
        for (i, &x) in input.iter().enumerate() {
            let y = spectral.process([x, -x], 0.0);
            if i > FFT_N * 2 {
                assert!(
                    (y[0] - input[i - FFT_N]).abs() < 0.00001,
                    "{i}: {} vs {}",
                    y[0],
                    input[i - FFT_N]
                );
            }
        }
    }
}
