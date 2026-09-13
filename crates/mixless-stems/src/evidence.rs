use crate::{models, Stems};
use mixless_protocol::{StemAnalysis, StemFrame, StemKind, StemNote};
use rustfft::{num_complex::Complex, FftPlanner};

pub const VERSION: u32 = 1;

pub fn extract(stems: &Stems, mut notes: Vec<StemNote>) -> StemAnalysis {
    const STEP: usize = 2205;
    const FFT: usize = 2048;
    let n = stems.audio[0].len() / 2;
    let fft = FftPlanner::new().plan_fft_forward(FFT);
    let mut scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
    let mut spectrum = vec![Complex::default(); FFT];
    let mut frames = Vec::new();
    let mut previous = [0f32; 3];
    let mut previous_low = 0.;
    for start in (0..n).step_by(STEP) {
        let end = (start + STEP).min(n);
        let mut frame = StemFrame {
            start_sec: start as f32 / 44100.,
            end_sec: end as f32 / 44100.,
            rms: [0.; 3],
            band_db: [[-120.; 3]; 3],
            onset: [0.; 3],
            drum_low_onset: 0.,
            vocal_activity: 0.,
            note_chroma: [0.; 12],
        };
        for stem in 0..3 {
            let samples = &stems.audio[stem][start * 2..end * 2];
            frame.rms[stem] = (samples.iter().map(|v| (*v as f64).powi(2)).sum::<f64>()
                / samples.len() as f64)
                .sqrt() as f32;
            frame.onset[stem] =
                ((frame.rms[stem] - previous[stem]) / frame.rms[stem].max(0.001)).clamp(0., 1.);
            previous[stem] = frame.rms[stem];
            for (i, v) in spectrum.iter_mut().enumerate() {
                let pair = samples.get(i * 2..i * 2 + 2).unwrap_or(&[0., 0.]);
                *v = Complex::new(
                    (pair[0] + pair[1])
                        * 0.5
                        * (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT as f32).cos()),
                    0.,
                );
            }
            fft.process_with_scratch(&mut spectrum, &mut scratch);
            let mut power = [0f32; 3];
            for (bin, value) in spectrum.iter().enumerate().take(FFT / 2).skip(1) {
                let hz = bin as f32 * 44100. / FFT as f32;
                power[if hz < 200. {
                    0
                } else if hz < 4000. {
                    1
                } else {
                    2
                }] += value.norm_sqr() / (FFT * FFT) as f32;
            }
            frame.band_db[stem] = power.map(|p| 10. * p.max(1e-12).log10());
            if stem == 1 {
                let low = power[0].sqrt();
                frame.drum_low_onset = ((low - previous_low) / low.max(0.001)).clamp(0., 1.);
                previous_low = low;
            }
        }
        // Both audibility and relative voice energy gate separation leakage.
        let v = frame.rms[0];
        let rest = frame.rms[1] + frame.rms[2];
        frame.vocal_activity = ((20. * v.max(1e-8).log10() + 55.) / 25.).clamp(0., 1.)
            * (v / (rest * 0.25 + 0.004)).clamp(0., 1.);
        frames.push(frame);
    }
    notes.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    for note in &notes {
        let stem = note.stem.index();
        if note.stem == StemKind::Drums {
            continue;
        }
        let first = frames.partition_point(|f| f.end_sec <= note.start_sec);
        for frame in frames[first..]
            .iter_mut()
            .take_while(|f| f.start_sec < note.end_sec)
        {
            let weight = note.confidence * (frame.rms[stem] * 10.).clamp(0., 1.);
            frame.note_chroma[note.midi as usize % 12] += weight;
        }
    }
    for frame in &mut frames {
        let norm = frame.note_chroma.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-6 {
            frame.note_chroma.iter_mut().for_each(|v| *v /= norm);
        }
    }
    StemAnalysis {
        version: VERSION,
        separator_sha256: models::SEPARATOR_HASH.into(),
        notes_sha256: models::NOTES_HASH.into(),
        duration_sec: n as f32 / 44100.,
        residual_rms: stems.residual_rms,
        frames,
        notes,
    }
}
