use std::{ffi::c_void, ptr::NonNull};

#[cfg(test)]
use crate::decode::AudioBuffer;
use crate::resample::Resampler;
use crate::source::SampleSource;

const OUTPUT_FRAMES: usize = 128;
const MAX_RATE: f64 = 4.0;

extern "C" {
    fn mixless_stretch_create(block: i32, interval: i32) -> *mut c_void;
    fn mixless_stretch_destroy(handle: *mut c_void);
    fn mixless_stretch_reset(handle: *mut c_void);
    fn mixless_stretch_input_latency(handle: *mut c_void) -> i32;
    fn mixless_stretch_output_latency(handle: *mut c_void) -> i32;
    fn mixless_stretch_pitch(handle: *mut c_void, semitones: f32);
    fn mixless_stretch_seek(handle: *mut c_void, input: *const f32, frames: i32, rate: f64);
    fn mixless_stretch_process(
        handle: *mut c_void,
        input: *const f32,
        input_frames: i32,
        output: *mut f32,
        output_frames: i32,
    );
}

#[derive(Clone, Copy)]
pub(crate) struct StretchParams {
    pub rate: f64,
    pub semitones: f32,
    pub source_step: f64,
    pub loop_range: Option<(f64, f64)>,
}

/// Stereo music path. The source cursor runs ahead of the audible transport;
/// returned source advances travel through the same output-latency queue.
/// Scratch gestures bypass this processor in the engine.
pub(crate) struct MusicStretch {
    handle: NonNull<c_void>,
    input_latency: usize,
    output_latency: usize,
    history_frames: usize,
    input: Vec<f32>,
    discard: Vec<f32>,
    output: [f32; OUTPUT_FRAMES * 2],
    output_index: usize,
    input_cursor: f64,
    remainder: f64,
    output_steps: [f64; OUTPUT_FRAMES],
    advance_history: Vec<f64>,
    history_index: usize,
    primed: bool,
}

// The native instance has exclusive ownership and is never accessed concurrently.
unsafe impl Send for MusicStretch {}

impl MusicStretch {
    pub fn new(sample_rate: f32) -> Self {
        // 64 ms analysis / 8 ms hop balances pitch resolution and control latency.
        // Split computation spreads spectral work over successive process calls.
        let block = ((sample_rate * 0.064 / 64.0).round() as usize).max(8) * 64;
        let interval = block / 8;
        let handle = NonNull::new(unsafe { mixless_stretch_create(block as i32, interval as i32) })
            .expect("allocate music stretcher");
        let input_latency = unsafe { mixless_stretch_input_latency(handle.as_ptr()) } as usize;
        let output_latency = unsafe { mixless_stretch_output_latency(handle.as_ptr()) } as usize;
        let capacity = (block + interval)
            .max(((output_latency + OUTPUT_FRAMES) as f64 * MAX_RATE).ceil() as usize + 1);
        let mut stretcher = Self {
            handle,
            input_latency,
            output_latency,
            history_frames: block + interval,
            input: vec![0.0; capacity * 2],
            discard: vec![0.0; output_latency * 2],
            output: [0.0; OUTPUT_FRAMES * 2],
            output_index: OUTPUT_FRAMES,
            input_cursor: 0.0,
            remainder: 0.0,
            output_steps: [1.0; OUTPUT_FRAMES],
            advance_history: vec![1.0; output_latency],
            history_index: 0,
            primed: false,
        };
        unsafe {
            mixless_stretch_process(
                handle.as_ptr(),
                stretcher.input.as_ptr(),
                capacity as i32,
                stretcher.discard.as_mut_ptr(),
                output_latency as i32,
            );
            mixless_stretch_reset(handle.as_ptr());
        }
        stretcher
    }

    pub fn invalidate(&mut self) {
        self.primed = false;
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    fn input_count(&mut self, output_frames: usize, rate: f64) -> usize {
        let exact = output_frames as f64 * rate + self.remainder;
        let frames = exact.floor() as usize;
        self.remainder = exact - frames as f64;
        frames
    }

    fn read_input(
        &mut self,
        buffer: &impl SampleSource,
        resampler: &Resampler,
        frames: usize,
        params: StretchParams,
    ) {
        // A cue at the output sample rate reads contiguous, integer-position
        // PCM while priming. Cubic interpolation at t=0 is the original sample;
        // copying the span avoids thousands of per-sample bounds/interpolation
        // operations on each retrigger without changing its audio or latency.
        if params.source_step == 1.
            && params.loop_range.is_none()
            && self.input_cursor >= 0.
            && self.input_cursor.fract() == 0.
            && self.input_cursor + frames as f64 <= buffer.frames() as f64
        {
            let first = self.input_cursor as usize * 2;
            if let Some(samples) = buffer.contiguous() {
                self.input[..frames * 2].copy_from_slice(&samples[first..first + frames * 2]);
            } else {
                for (i, frame) in self.input[..frames * 2].chunks_exact_mut(2).enumerate() {
                    frame.copy_from_slice(&buffer.sample(first / 2 + i));
                }
            }
            self.input_cursor += frames as f64;
            return;
        }
        for frame in 0..frames {
            let position = if let Some((start, end)) = params.loop_range {
                start + (self.input_cursor - start).rem_euclid(end - start)
            } else {
                self.input_cursor
            };
            let (left, right) = resampler.stereo_at(buffer, position, params.source_step);
            self.input[frame * 2] = left;
            self.input[frame * 2 + 1] = right;
            self.input_cursor += params.source_step;
        }
    }

    fn prime(
        &mut self,
        buffer: &impl SampleSource,
        resampler: &Resampler,
        position: f64,
        params: StretchParams,
    ) {
        self.remainder = 0.0;
        self.advance_history.fill(params.source_step * params.rate);
        self.history_index = 0;
        // Center analysis on the requested source position, then discard the
        // synthesis delay. Local decoded audio permits this lookahead without
        // moving the audible transport ahead on Cue or scratch release.
        self.input_cursor = position
            + (self.input_latency as f64 - self.history_frames as f64) * params.source_step;
        self.read_input(buffer, resampler, self.history_frames, params);
        unsafe {
            mixless_stretch_reset(self.handle.as_ptr());
            mixless_stretch_pitch(self.handle.as_ptr(), params.semitones);
            mixless_stretch_seek(
                self.handle.as_ptr(),
                self.input.as_ptr(),
                self.history_frames as i32,
                params.rate,
            );
        }
        let input_frames = self.input_count(self.output_latency, params.rate);
        self.read_input(buffer, resampler, input_frames, params);
        unsafe {
            mixless_stretch_process(
                self.handle.as_ptr(),
                self.input.as_ptr(),
                input_frames as i32,
                self.discard.as_mut_ptr(),
                self.output_latency as i32,
            );
        }
        self.output_index = OUTPUT_FRAMES;
        self.primed = true;
    }

    pub fn next(
        &mut self,
        buffer: &impl SampleSource,
        resampler: &Resampler,
        position: f64,
        params: StretchParams,
    ) -> ([f32; 2], f64) {
        if !self.primed {
            self.prime(buffer, resampler, position, params);
        }
        if self.output_index == OUTPUT_FRAMES {
            let input_frames = self.input_count(OUTPUT_FRAMES, params.rate);
            self.read_input(buffer, resampler, input_frames, params);
            unsafe {
                mixless_stretch_pitch(self.handle.as_ptr(), params.semitones);
                mixless_stretch_process(
                    self.handle.as_ptr(),
                    self.input.as_ptr(),
                    input_frames as i32,
                    self.output.as_mut_ptr(),
                    OUTPUT_FRAMES as i32,
                );
            }
            let step = params.source_step * input_frames as f64 / OUTPUT_FRAMES as f64;
            for output_step in &mut self.output_steps {
                *output_step = self.advance_history[self.history_index];
                self.advance_history[self.history_index] = step;
                self.history_index = (self.history_index + 1) % self.advance_history.len();
            }
            self.output_index = 0;
        }
        let index = self.output_index * 2;
        self.output_index += 1;
        (
            [self.output[index], self.output[index + 1]],
            self.output_steps[self.output_index - 1],
        )
    }
}

impl Drop for MusicStretch {
    fn drop(&mut self) {
        unsafe { mixless_stretch_destroy(self.handle.as_ptr()) };
    }
}

#[cfg(test)]
pub(crate) fn frequency(samples: &[f32], sample_rate: f64) -> f64 {
    let mut first = None;
    let mut last = 0.0;
    let mut crossings = 0;
    for (index, pair) in samples.windows(2).enumerate() {
        if pair[0] <= 0.0 && pair[1] > 0.0 {
            let position = index as f64 - pair[0] as f64 / (pair[1] - pair[0]) as f64;
            first.get_or_insert(position);
            last = position;
            crossings += 1;
        }
    }
    if crossings < 2 {
        return 0.0;
    }
    let estimate = (crossings - 1) as f64 * sample_rate / (last - first.unwrap());
    let count = samples.len().min(32768);
    let weighted: Vec<f64> = samples[..count]
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            *sample as f64
                * (0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / (count - 1) as f64).cos())
        })
        .collect();
    let power = |frequency: f64| {
        let coefficient = 2.0 * (std::f64::consts::TAU * frequency / sample_rate).cos();
        let (mut previous, mut before) = (0.0, 0.0);
        for sample in &weighted {
            let value = sample + coefficient * previous - before;
            before = previous;
            previous = value;
        }
        previous * previous + before * before - coefficient * previous * before
    };
    let mut best_frequency = estimate;
    let mut best_power = 0.0;
    for frequency in (estimate * 0.9).floor() as usize..=(estimate * 1.1).ceil() as usize {
        let candidate = power(frequency as f64);
        if candidate > best_power {
            best_power = candidate;
            best_frequency = frequency as f64;
        }
    }
    let center = best_frequency;
    for offset in -20..=20 {
        let frequency = center + offset as f64 * 0.05;
        let candidate = power(frequency);
        if candidate > best_power {
            best_power = candidate;
            best_frequency = frequency;
        }
    }
    best_frequency
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contiguous_cue_input_matches_interpolation_and_keeps_loop_and_edge_reads() {
        let buffer = tone(48000);
        let resampler = Resampler::new();
        let mut stretch = MusicStretch::new(48000.);
        for (start, step, loop_range) in [
            (24000., 1., None),
            (-10., 1., None),
            (buffer.frames as f64 - 50., 1., None),
            (24000.5, 44100. / 48000., None),
            (30., 1., Some((20., 50.))),
        ] {
            let params = StretchParams {
                rate: 1.08,
                semitones: 2.,
                source_step: step,
                loop_range,
            };
            stretch.input_cursor = start;
            stretch.read_input(&buffer, &resampler, 128, params);
            let mut cursor = start;
            for i in 0..128 {
                let source =
                    loop_range.map_or(cursor, |(lo, hi)| lo + (cursor - lo).rem_euclid(hi - lo));
                let (left, right) = resampler.stereo_at(&buffer, source, step);
                assert_eq!(stretch.input[i * 2], left);
                assert_eq!(stretch.input[i * 2 + 1], right);
                cursor += step;
            }
            assert_eq!(stretch.input_cursor, cursor);
        }
    }

    fn tone(sample_rate: u32) -> AudioBuffer {
        let frames = sample_rate as usize * 4;
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let sample = (std::f64::consts::TAU * 440.0 * frame as f64 / sample_rate as f64).sin()
                as f32
                * 0.5;
            samples.extend_from_slice(&[sample, sample]);
        }
        AudioBuffer {
            loudness: Default::default(),
            samples,
            frames: frames as u64,
            sample_rate,
        }
    }

    #[test]
    fn tempo_and_key_are_independent_across_sample_rates() {
        for (source_rate, output_rate) in [
            (44100, 48000),
            (48000, 48000),
            (96000, 48000),
            (48000, 96000),
        ] {
            let buffer = tone(source_rate);
            let resampler = Resampler::new();
            for (rate, semitones) in [
                (0.88, 0.0),
                (1.12, 0.0),
                (0.5, 12.0),
                (1.5, -12.0),
                (1.0, 7.0),
                (1.0, -5.0),
            ] {
                let mut stretch = MusicStretch::new(output_rate as f32);
                let params = StretchParams {
                    rate,
                    semitones,
                    source_step: source_rate as f64 / output_rate as f64,
                    loop_range: None,
                };
                let start = source_rate as f64 * 0.25;
                let mut position = start;
                let mut samples = Vec::new();
                let mut stereo_error = 0.0;
                let mut peak_stereo_error = 0.0f32;
                for frame in 0..output_rate {
                    let (sample, advance) = stretch.next(&buffer, &resampler, position, params);
                    position += advance;
                    assert!(sample.iter().all(|value| value.is_finite()));
                    if frame >= output_rate / 4 {
                        samples.push(sample[0]);
                        stereo_error += (sample[0] - sample[1]).powi(2);
                        peak_stereo_error = peak_stereo_error.max((sample[0] - sample[1]).abs());
                    }
                }
                let measured = frequency(&samples, output_rate as f64);
                let expected = 440.0 * 2.0f64.powf(semitones as f64 / 12.0);
                assert!((stereo_error / samples.len() as f32).sqrt() < 0.001,
                    "stereo: src={source_rate} dst={output_rate} rate={rate} key={semitones} rms={} peak={peak_stereo_error}", (stereo_error / samples.len() as f32).sqrt());
                let tolerance = if (0.88..=1.12).contains(&rate) && semitones.abs() <= 7.0 {
                    0.002
                } else {
                    0.005
                };
                assert!((measured - expected).abs() < expected * tolerance, "src={source_rate} dst={output_rate} rate={rate} key={semitones} measured={measured} expected={expected}");
                assert!(
                    (position - start - source_rate as f64 * rate).abs()
                        < source_rate as f64 / output_rate as f64 + 0.01
                );
            }
        }
    }

    #[test]
    fn fractional_consumption_does_not_drift() {
        let buffer = tone(48000);
        let resampler = Resampler::new();
        let mut stretch = MusicStretch::new(48000.0);
        let params = StretchParams {
            rate: 1.00337,
            semitones: 0.0,
            source_step: 1.0,
            loop_range: Some((0.0, buffer.frames as f64)),
        };
        let mut position = 0.0;
        for _ in 0..480_000 {
            let (_, step) = stretch.next(&buffer, &resampler, position, params);
            position += step;
        }
        assert!((position - 480_000.0 * params.rate).abs() < 1.0);
    }

    #[test]
    fn tempo_automation_delays_transport_metadata_with_audio_pipeline() {
        let buffer = tone(48000);
        let resampler = Resampler::new();
        for direction in [1.0, -1.0] {
            let mut stretch = MusicStretch::new(48000.0);
            let rate_at = |frame| {
                if frame < OUTPUT_FRAMES * 10 + 17 {
                    0.875
                } else if frame < OUTPUT_FRAMES * 35 + 3 {
                    1.125
                } else {
                    0.75
                }
            };
            let mut position = 96000.0;
            for frame in 0..12000 {
                let params = StretchParams {
                    rate: rate_at(frame),
                    semitones: 3.0,
                    source_step: direction,
                    loop_range: None,
                };
                let (sample, step) = stretch.next(&buffer, &resampler, position, params);
                // Parameters are sampled when each 128-frame chunk is rendered,
                // and its transport metadata follows the output latency, not the
                // latest UI rate. Changes deliberately land mid-chunk.
                let audible_rate = if frame < stretch.output_latency {
                    rate_at(0)
                } else {
                    rate_at((frame - stretch.output_latency) / OUTPUT_FRAMES * OUTPUT_FRAMES)
                };
                assert!(
                    (step - audible_rate * direction).abs() < 1e-9,
                    "frame={frame} direction={direction} step={step} expected={audible_rate}"
                );
                assert!(sample.iter().all(|value| value.is_finite()));
                position += step;
            }
        }
    }

    #[test]
    fn preroll_aligns_unity_audio_with_requested_position() {
        let buffer = tone(48000);
        let resampler = Resampler::new();
        let mut stretch = MusicStretch::new(48000.0);
        let params = StretchParams {
            rate: 1.0,
            semitones: 0.0,
            source_step: 1.0,
            loop_range: None,
        };
        let mut position = 25000.0;
        let mut error = 0.0;
        for _ in 0..4800 {
            let (sample, step) = stretch.next(&buffer, &resampler, position, params);
            let expected = buffer.stereo_at(position).0;
            error += (sample[0] - expected).powi(2);
            position += step;
        }
        assert!(
            (error / 4800.0).sqrt() < 0.03,
            "alignment RMS error {}",
            (error / 4800.0).sqrt()
        );
    }
}

/// Fixed-rate streaming insert pitch shifter. Equal input/output counts keep
/// deck time unchanged; the wet signal carries the processor's own latency.
pub(crate) struct InsertPitch {
    handle: NonNull<c_void>,
    input: [f32; OUTPUT_FRAMES * 2],
    output: [f32; OUTPUT_FRAMES * 2],
    index: usize,
}
unsafe impl Send for InsertPitch {}
impl InsertPitch {
    pub fn new(sr: f32) -> Self {
        let block = ((sr * 0.032 / 64.0).round() as usize).max(8) * 64;
        let handle =
            NonNull::new(unsafe { mixless_stretch_create(block as i32, (block / 4) as i32) })
                .expect("allocate insert pitch shifter");
        let mut pitch = Self {
            handle,
            input: [0.0; OUTPUT_FRAMES * 2],
            output: [0.0; OUTPUT_FRAMES * 2],
            index: 0,
        };
        // Exercise every internal phase before starting the device callback.
        for _ in 0..block * 3 {
            pitch.process([0.0; 2]);
        }
        pitch.reset();
        pitch
    }
    pub fn configure(&mut self, semitones: f32) {
        unsafe {
            mixless_stretch_pitch(self.handle.as_ptr(), semitones);
        }
    }
    pub fn reset(&mut self) {
        unsafe {
            mixless_stretch_reset(self.handle.as_ptr());
        }
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.index = 0;
    }
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let i = self.index * 2;
        let out = [self.output[i], self.output[i + 1]];
        self.input[i] = input[0];
        self.input[i + 1] = input[1];
        self.index += 1;
        if self.index == OUTPUT_FRAMES {
            unsafe {
                mixless_stretch_process(
                    self.handle.as_ptr(),
                    self.input.as_ptr(),
                    OUTPUT_FRAMES as i32,
                    self.output.as_mut_ptr(),
                    OUTPUT_FRAMES as i32,
                );
            }
            self.index = 0;
        }
        out
    }
}
impl Drop for InsertPitch {
    fn drop(&mut self) {
        unsafe {
            mixless_stretch_destroy(self.handle.as_ptr());
        }
    }
}
