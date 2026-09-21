mod moments;
mod tempo;
mod tonal;
use tempo::tempo;

use mixless_protocol::{
    BarFeature, SectionLabel as S, TempoMap, TempoSegment, TrackAnalysis, TrackId,
};
use rustfft::{num_complex::Complex, FftPlanner};
const SR: f32 = 22050.;
const FFT: usize = 2048;
const HOP: usize = 256;
// Bump whenever bar structure labels or cue-facing features change; cached
// analyses from older classifiers must not drive Automix.
pub const ANALYSIS_VERSION: u32 = 18;

#[derive(Default, Clone)]
struct Frame {
    time: f32,
    rms: f32,
    peak: f32,
    band: [f32; 3],
    chroma: [f32; 12],
    onset: f32,
    low_onset: f32,
    tonal_risk: f32,
    pitch_midi: Option<f32>,
}

pub(crate) fn analyze(id: TrackId, stereo: &[f32], sr: u32) -> TrackAnalysis {
    let duration = stereo.len() as f32 / 2. / sr.max(1) as f32;
    let mut mono = Vec::with_capacity((duration * SR) as usize);
    let (mut left, mut right, mut combined) = (0f64, 0f64, 0f64);
    for frame in stereo.chunks_exact(2) {
        left += (frame[0] as f64).powi(2);
        right += (frame[1] as f64).powi(2);
        combined += ((frame[0] as f64 + frame[1] as f64) * 0.5).powi(2);
    }
    let dominant = (combined < left.max(right) * 0.01).then_some(if left >= right { 0 } else { 1 });
    // Area downsampling averages each source interval, preserving DC and RMS
    // envelopes; spectral features above 8 kHz are deliberately discarded.
    for i in 0..(duration * SR) as usize {
        let a = (i as f64 * sr as f64 / SR as f64) as usize;
        let b = (((i + 1) as f64 * sr as f64 / SR as f64) as usize)
            .max(a + 1)
            .min(stereo.len() / 2);
        let mut sum = 0.;
        for j in a..b {
            sum += dominant.map_or_else(
                || (stereo[j * 2] + stereo[j * 2 + 1]) * 0.5,
                |c| stereo[j * 2 + c],
            );
        }
        mono.push(if b > a { sum / (b - a) as f32 } else { 0. });
    }
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT);
    let window: Vec<_> = (0..FFT)
        .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / FFT as f32).cos())
        .collect();
    let mut complex = vec![Complex::new(0., 0.); FFT];
    let mut scratch = vec![Complex::new(0., 0.); fft.get_inplace_scratch_len()];
    let mut previous = vec![0.; FFT / 2];
    let mut previous_low = 0.;
    let mut frames = Vec::new();
    let bins: Vec<_> = (2..FFT / 2 - 1)
        .map(|k| {
            let hz = k as f32 * SR / FFT as f32;
            (
                k,
                hz,
                if hz < 150. {
                    0
                } else if hz < 2000. {
                    1
                } else {
                    2
                },
                ((69. + 12. * (hz / 440.).log2()).round() as i32).rem_euclid(12) as usize,
            )
        })
        .take_while(|(_, hz, _, _)| *hz <= 8000.)
        .collect();
    for start in (0..mono.len().saturating_sub(FFT)).step_by(HOP) {
        let mut frame = Frame {
            time: (start + FFT / 2) as f32 / SR,
            ..Default::default()
        };
        for n in 0..FFT {
            let x = mono[start + n];
            frame.rms += x * x;
            frame.peak = frame.peak.max(x.abs());
            complex[n] = Complex::new(x * window[n], 0.);
        }
        frame.rms = (frame.rms / FFT as f32).sqrt();
        fft.process_with_scratch(&mut complex, &mut scratch);
        let mut harmonic = 0.;
        let mut ridge_power = 0.;
        let mut total = 0.;
        for &(k, hz, band, pc) in &bins {
            let power = complex[k].norm_sqr();
            let mag = power.sqrt();
            frame.band[band] += power;
            total += power;
            frame.onset += (mag - previous[k]).max(0.);
            previous[k] = mag;
            if (100.0..4000.0).contains(&hz)
                && power > complex[k - 1].norm_sqr() * 1.5
                && power > complex[k + 1].norm_sqr() * 1.5
            {
                frame.chroma[pc] += power;
                if hz >= 180. && hz <= 2500. && power > ridge_power {
                    ridge_power = power;
                    let l = complex[k - 1].norm().max(1e-12).ln();
                    let c = mag.max(1e-12).ln();
                    let r = complex[k + 1].norm().max(1e-12).ln();
                    let delta = (0.5 * (l - r) / (l - 2. * c + r).min(-1e-6)).clamp(-0.5, 0.5);
                    frame.pitch_midi =
                        Some(69. + 12. * (((k as f32 + delta) * SR / FFT as f32) / 440.).log2());
                }
                if hz > 180. && hz < 3000. {
                    harmonic += power;
                }
            }
        }
        frame.low_onset = (frame.band[0].sqrt() - previous_low).max(0.);
        previous_low = frame.band[0].sqrt();
        // Risk of sustained foreground (voice OR lead instrument), not a claim
        // of source separation. False positives intentionally shorten overlaps.
        frame.tonal_risk = (harmonic / total.max(1e-12) * 2.).clamp(0., 1.);
        let total_chroma = frame.chroma.iter().sum::<f32>();
        if total_chroma > 0. {
            for c in &mut frame.chroma {
                *c /= total_chroma;
            }
        }
        frames.push(frame);
    }
    let (bpm, phase, confidence) = tempo(&frames, duration);
    let tonal = tonal::analyze(&mono);
    let (key, camelot, key_confidence) = tonal.key.clone();
    // Estimate tempo independently for each phrase. A single global BPM is
    // retained as a fallback, while the beat timestamps follow the local
    // periods so accelerando/section changes do not accumulate phase error.
    let phrase_beats = 16usize;
    let mut local_bpms = Vec::new();
    let mut local_confs = Vec::new();
    let mut probe = phase;
    while probe < duration {
        let end = (probe + phrase_beats as f32 * 60.0 / bpm).min(duration);
        let start = frames.partition_point(|f| f.time < probe);
        let stop = frames.partition_point(|f| f.time < end);
        let local = &frames[start..stop];
        let (estimate, _, local_confidence) = if local.len() >= 32 {
            tempo(local, (end - probe).max(1.0))
        } else {
            (bpm, phase, 0.)
        };
        let norm_conf = local_confidence / ((end - probe) / 16.).clamp(0.01, 1.);
        let local_bpm = if estimate.is_finite() {
            // Autocorrelation can select a metrical alias in a short phrase.
            // Snap extreme outliers to the closest sub/harmonic of the global
            // fit while preserving genuine local changes such as 100 -> 80 BPM.
            let corrected = if estimate < bpm * 0.6 || estimate > bpm * 1.6 {
                [0.5, 2., 2.5, 3., 4.]
                    .iter()
                    .map(|f| estimate * f)
                    .min_by(|a, b| (a / bpm - 1.).abs().total_cmp(&(b / bpm - 1.).abs()))
                    .unwrap_or(estimate)
            } else {
                estimate
            };
            if norm_conf < 0.55 || (corrected / bpm - 1.).abs() < 0.015 {
                bpm
            } else {
                corrected.clamp(70., 180.)
            }
        } else {
            bpm
        };
        local_bpms.push(local_bpm);
        local_confs.push(norm_conf);
        // Keep the next analysis window on the same phrase boundary as the
        // generated beat grid. This prevents a tempo change from shifting the
        // segment-to-source mapping after the first phrase.
        probe += phrase_beats as f32 * 60.0 / local_bpm;
    }
    if local_bpms.is_empty() {
        local_bpms.push(bpm);
        local_confs.push(0.);
    }
    // A lone 16-beat estimate that disagrees with its neighbours is an
    // autocorrelation artifact, not a musical change: one wrong period
    // phase-shifts every later beat. Keep a deviation only when at least two
    // adjacent windows agree on it with measured confidence. Metrical aliases
    // of the global tempo are never sustained changes.
    let len = local_bpms.len();
    let mut reverted = vec![false; len];
    for i in 0..len {
        let v = local_bpms[i];
        if (v / bpm - 1.).abs() < 0.015 {
            continue;
        }
        let ratio = v / bpm;
        let alias = [2. / 3., 0.75, 4. / 3., 1.5]
            .iter()
            .any(|r| (ratio / r - 1.).abs() < 0.04);
        let sustained = [i.wrapping_sub(1), i + 1].iter().any(|&j| {
            j < len
                && j != i
                && (local_bpms[j] / v - 1.).abs() < 0.015
                && local_confs[i] >= 0.55
                && local_confs[j] >= 0.55
        });
        reverted[i] = alias || !sustained;
    }
    for i in 0..len {
        if !reverted[i] {
            continue;
        }
        // A rejected window between two regions that agree with each other
        // belongs to their shared tempo; otherwise the global fit is the
        // safest reading.
        let left = (0..i).rev().find(|&j| !reverted[j]).map(|j| local_bpms[j]);
        let right = (i + 1..len).find(|&j| !reverted[j]).map(|j| local_bpms[j]);
        local_bpms[i] = match (left, right) {
            (Some(l), Some(r)) if (l / r - 1.).abs() < 0.015 => l,
            _ => bpm,
        };
    }
    // When the filtered estimates all agree with the global fit the track is
    // constant-tempo: one period and one phase, so neither a confident
    // outlier nor a drum-less break can phase-shift the rest of the grid.
    let constant = local_bpms.iter().all(|&v| (v / bpm - 1.).abs() < 0.015);
    if constant {
        local_bpms.iter_mut().for_each(|v| *v = bpm);
    }
    let mut beats = Vec::new();
    let mut t = phase;
    let mut beat_index = 0usize;
    while t <= duration {
        beats.push(t);
        let phrase = (beat_index / phrase_beats).min(local_bpms.len() - 1);
        t += 60.0 / local_bpms[phrase];
        beat_index += 1;
    }
    // Downbeat phase is estimated from the strongest recurring low-frequency
    // accent. Confidence includes accent ambiguity; a uniform click is not
    // enough evidence for a musical bar-one.
    let mut accents = [0.; 4];
    for (i, &beat) in beats.iter().enumerate() {
        let start = frames.partition_point(|f| f.time < beat - 0.06);
        let end = frames.partition_point(|f| f.time <= beat + 0.06);
        accents[i % 4] += frames[start..end]
            .iter()
            .map(|f| f.low_onset)
            .fold(0., f32::max);
    }
    let downbeat = (0..4)
        .max_by(|a, b| accents[*a].total_cmp(&accents[*b]))
        .unwrap_or(0);
    let mut downbeats: Vec<_> = beats.iter().skip(downbeat).step_by(4).copied().collect();
    if downbeats.is_empty() {
        downbeats.push(phase.min(duration));
    }
    let runner_up = accents
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != downbeat)
        .map(|(_, v)| *v)
        .fold(0., f32::max);
    let accent_confidence =
        ((accents[downbeat] - runner_up) / accents[downbeat].max(1e-6) / 0.3).clamp(0., 1.);
    // A regular click alone cannot identify bar one. Also validate alignment
    // locally so a constant grid fitted to a tempo-changing song is never
    // advertised as reliable throughout that song.
    let mut segments = Vec::new();
    let mut pulse_confidence = Vec::new();
    // Constant-tempo grids validate alignment once, globally: a quiet break
    // then reports the measured global pulse instead of a meaningless local
    // zero, and per-window alignment cannot veto an already fitted grid.
    let global_pulse = constant.then(|| tempo::pulse_confidence(&frames, bpm, phase));
    for first in (0..beats.len()).step_by(phrase_beats) {
        let last = (first + phrase_beats).min(beats.len());
        let local_bpm = local_bpms[(first / phrase_beats).min(local_bpms.len() - 1)];
        let local_phase = beats[first];
        let start = frames.partition_point(|f| f.time < beats[first]);
        let end_time = beats.get(last).copied().unwrap_or(duration);
        let end = frames.partition_point(|f| f.time < end_time);
        let local = &frames[start..end];
        let pulse = tempo::pulse_confidence(local, local_bpm, local_phase);
        pulse_confidence.push(global_pulse.map_or(pulse, |g| pulse.max(g)));
        let mut conf = confidence.min(accent_confidence);
        if !constant {
            conf = conf.min(tempo::alignment_confidence(local, local_bpm, local_phase));
        }
        segments.push(TempoSegment {
            start_beat: first as f32,
            end_beat: last as f32,
            bpm: local_bpm,
            confidence: conf,
        });
    }
    if segments.is_empty() {
        segments.push(TempoSegment {
            start_beat: 0.,
            end_beat: 1.,
            bpm,
            confidence: 0.,
        });
    }
    let mut bars = Vec::new();
    let mut boundaries = downbeats.clone();
    if boundaries.first().is_some_and(|v| *v > 0.) {
        boundaries.insert(0, 0.);
    }
    if boundaries.last().is_none_or(|v| *v < duration) {
        boundaries.push(duration);
    }
    let onset_max = frames.iter().map(|f| f.onset).fold(0., f32::max).max(1e-6);
    let low_max = frames
        .iter()
        .map(|f| f.low_onset)
        .fold(0., f32::max)
        .max(1e-6);
    let mut cursor = 0;
    for (index, p) in boundaries.windows(2).enumerate() {
        while cursor < frames.len() && frames[cursor].time < p[0] {
            cursor += 1;
        }
        let first = cursor;
        while cursor < frames.len() && frames[cursor].time < p[1] {
            cursor += 1;
        }
        let selected = &frames[first..cursor];
        let count = selected.len().max(1) as f32;
        let rms = (selected.iter().map(|f| f.rms * f.rms).sum::<f32>() / count).sqrt();
        let peak = selected.iter().map(|f| f.peak).fold(0., f32::max);
        let mut band = [0.; 3];
        let chroma = tonal.chroma(p[0], p[1]);
        for f in selected {
            for i in 0..3 {
                band[i] += f.band[i];
            }
        }
        let total = band.iter().sum::<f32>().max(1e-12);
        let db = |share: f32| 10. * (rms * rms * share / total).max(1e-12).log10();
        let attacks = selected
            .iter()
            .filter(|f| f.onset > onset_max * 0.12)
            .count() as f32;
        let risk = selected.iter().filter(|f| f.tonal_risk > 0.3).count() as f32 / count;
        bars.push(BarFeature {
            bar_index: index as u32,
            start_sec: p[0],
            end_sec: p[1],
            rms,
            crest: peak / rms.max(1e-6),
            low_db: db(band[0]),
            mid_db: db(band[1]),
            high_db: db(band[2]),
            chroma,
            chord: None,
            local_key: None,
            onset_density: attacks / (p[1] - p[0]).max(0.01),
            kick_salience: (selected.iter().map(|f| f.low_onset / low_max).sum::<f32>() / 4.)
                .clamp(0., 1.),
            hat_salience: (band[2] / total).sqrt(),
            vocal_presence: risk,
            vocal_confidence: None,
            energy_slope: 0.,
            section: S::Unknown,
        });
    }
    let (sections, phrase_boundaries) =
        crate::structure::detect(&mut bars, if confidence >= 0.1 { &downbeats } else { &[] });
    TrackAnalysis {
        track_id: id,
        duration_sec: duration,
        sample_rate: sr,
        tempo: TempoMap {
            // Keep the globally fitted BPM stable for library display and
            // fallback consumers. Runtime timing uses the local grid segments.
            global_bpm: bpm,
            meter_num: 4,
            meter_den: 4,
            segments,
            pulse_confidence,
            // A fallback BPM is useful to the conservative mix planner, but
            // silence/aperiodic audio must not advertise an invented beat grid
            // to the waveform or Beat Sync. Bar-one confidence is independent.
            beats: if confidence >= 0.1 { beats } else { vec![] },
            downbeats: if confidence >= 0.1 { downbeats } else { vec![] },
        },
        key,
        camelot,
        key_confidence,
        sections,
        bars,
        phrase_boundaries,
        mix_regions: vec![],
        moments: moments::extract(&frames, duration),
        stems: None,
        waveform_path: None,
        partial: true,
    }
}

pub(crate) use mixless_protocol::estimate_key as key;

#[cfg(test)]
mod tests;
