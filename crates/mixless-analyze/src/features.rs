use mixless_protocol::{
    BarFeature, SectionLabel as S, TempoMap, TempoSegment, TrackAnalysis, TrackId,
};
use rustfft::{FftPlanner, num_complex::Complex};
const SR: f32 = 22050.;
const FFT: usize = 2048;
const HOP: usize = 256;
// Bump whenever bar structure labels or cue-facing features change; cached
// analyses from older classifiers must not drive Automix.
pub const ANALYSIS_VERSION: u32 = 8;

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
    let mut global = [0f32; 12];
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
                global[pc] += power;
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
    let (key, camelot, key_confidence) = key(&global);
    // Estimate tempo independently for each phrase. A single global BPM is
    // retained as a fallback, while the beat timestamps follow the local
    // periods so accelerando/section changes do not accumulate phase error.
    let phrase_beats = 16usize;
    let mut local_bpms = Vec::new();
    let mut probe = phase;
    while probe < duration {
        let end = (probe + phrase_beats as f32 * 60.0 / bpm).min(duration);
        let start = frames.partition_point(|f| f.time < probe);
        let stop = frames.partition_point(|f| f.time < end);
        let local = &frames[start..stop];
        let estimate = if local.len() >= 32 {
            tempo(local, (end - probe).max(1.0)).0
        } else {
            bpm
        };
        let local_bpm = if estimate.is_finite() {
            // Autocorrelation can select a half/double-time alias in a short
            // phrase. Correct clear aliases while preserving genuine local
            // changes such as 100 -> 80 BPM.
            let corrected = if estimate < bpm * 0.6 {
                estimate * 2.0
            } else if estimate > bpm * 1.6 {
                estimate * 0.5
            } else {
                estimate
            };
            corrected.clamp(70., 180.)
        } else {
            bpm
        };
        local_bpms.push(local_bpm);
        // Keep the next analysis window on the same phrase boundary as the
        // generated beat grid. This prevents a tempo change from shifting the
        // segment-to-source mapping after the first phrase.
        probe += phrase_beats as f32 * 60.0 / local_bpm;
    }
    if local_bpms.is_empty() {
        local_bpms.push(bpm);
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
    for first in (0..beats.len()).step_by(phrase_beats) {
        let last = (first + phrase_beats).min(beats.len());
        let local_bpm = local_bpms[(first / phrase_beats).min(local_bpms.len() - 1)];
        let local_period = 60.0 / local_bpm;
        let local_phase = beats[first];
        let start = frames.partition_point(|f| f.time < beats[first]);
        let end_time = beats.get(last).copied().unwrap_or(duration);
        let end = frames.partition_point(|f| f.time < end_time);
        let local = &frames[start..end];
        let total: f32 = local.iter().map(|f| f.onset).sum();
        let aligned: f32 = local
            .iter()
            .filter(|f| {
                let p = (f.time - local_phase).rem_euclid(local_period);
                p.min(local_period - p) < 0.035
            })
            .map(|f| f.onset)
            .sum();
        segments.push(TempoSegment {
            start_beat: first as f32,
            end_beat: last as f32,
            bpm: local_bpm,
            confidence: confidence
                .min(accent_confidence)
                .min((aligned / total.max(1e-6) * 1.5).clamp(0., 1.)),
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
        let mut chroma = [0.; 12];
        for f in selected {
            for i in 0..3 {
                band[i] += f.band[i];
            }
            for i in 0..12 {
                chroma[i] += f.chroma[i] / count;
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
        waveform_path: None,
        partial: true,
    }
}

fn tempo(frames: &[Frame], duration: f32) -> (f32, f32, f32) {
    if frames.len() < 128 {
        return (120., 0., 0.);
    }
    let flux: Vec<_> = frames.iter().map(|f| f.onset).collect();
    let dt = HOP as f32 / SR;
    let norm = flux.iter().map(|v| v * v).sum::<f32>();
    if norm < 1e-6 {
        return (120., 0., 0.);
    }
    let corr = |lag: usize| {
        flux.iter()
            .skip(lag)
            .zip(&flux)
            .map(|(a, b)| a * b)
            .sum::<f32>()
            / (norm * (1. - lag as f32 / flux.len() as f32)).max(1e-6)
    };
    let min = (60. / 180. / dt) as usize;
    let max = (60. / 70. / dt).ceil() as usize;
    // Compute each autocorrelation once, including across phrase probes.
    let correlations: Vec<_> = (min - 1..=max + 1).map(corr).collect();
    let best = (min..=max)
        .max_by(|a, b| correlations[*a - min + 1].total_cmp(&correlations[*b - min + 1]))
        .unwrap_or(min);
    let at = best - min + 1;
    let (left, mid, right) = (correlations[at - 1], correlations[at], correlations[at + 1]);
    let offset = (0.5 * (left - right) / (left - 2. * mid + right).min(-1e-6)).clamp(-0.5, 0.5);
    let mut bpm = 60. / ((best as f32 + offset) * dt);
    // Grid-search phase and tempo across the whole recording; a coarse integer
    // autocorrelation lag alone drifts by an audible fraction of a beat.
    let mut fit = 0.;
    let mut phase = 0.;
    let center = bpm;
    for step in -12..=12 {
        let candidate = center + step as f32 * 0.05;
        let period = 60. / candidate;
        let bins = 64;
        let mut histogram = [0.; 64];
        for f in frames {
            let p = ((f.time / period).fract() * bins as f32) as usize;
            histogram[p.min(63)] += f.onset;
        }
        for j in 0..bins {
            let score = histogram[j] + 0.5 * (histogram[(j + 63) % 64] + histogram[(j + 1) % 64]);
            if score > fit {
                fit = score;
                bpm = candidate;
                phase = (j as f32 + 0.5) / bins as f32 * period;
            }
        }
    }
    let period = 60. / bpm;
    let total = flux.iter().sum::<f32>().max(1e-6);
    let aligned = frames
        .iter()
        .filter(|f| {
            let p = (f.time - phase).rem_euclid(period);
            p.min(period - p) < 0.035
        })
        .map(|f| f.onset)
        .sum::<f32>()
        / total;
    let confidence = (aligned * 1.5).min(mid.max(0.)).clamp(0., 1.) * (duration / 16.).min(1.);
    (bpm.clamp(70., 180.), phase, confidence)
}
pub(crate) fn key(chroma: &[f32; 12]) -> (Option<String>, Option<String>, f32) {
    let sum = chroma.iter().sum::<f32>();
    if sum < 1e-8 {
        return (None, None, 0.);
    }
    let major = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    let minor = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let cams = [
        [
            "8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B",
        ],
        [
            "5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A",
        ],
    ];
    let mut scores = Vec::new();
    let mean = sum / 12.;
    for (mode, template) in [major, minor].iter().enumerate() {
        let tm = template.iter().sum::<f32>() / 12.;
        for root in 0..12 {
            let mut dot = 0.;
            let mut aa = 0.;
            let mut bb = 0.;
            for i in 0..12 {
                let a = chroma[i] - mean;
                let b = template[(i + 12 - root) % 12] - tm;
                dot += a * b;
                aa += a * a;
                bb += b * b;
            }
            scores.push((dot / (aa * bb).sqrt().max(1e-8), mode, root));
        }
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (score, mode, root) = scores[0];
    let confidence = (score.max(0.) * ((score - scores[1].0).max(0.) * 6.).min(1.)).clamp(0., 1.);
    (
        Some(format!(
            "{} {}",
            names[root],
            if mode == 0 { "major" } else { "minor" }
        )),
        Some(cams[mode][root].into()),
        confidence,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drums(sr: u32, bpm: f32, seconds: f32) -> Vec<f32> {
        let mut samples = Vec::new();
        for i in 0..(sr as f32 * seconds) as usize {
            let t = i as f32 / sr as f32;
            let beat = t * bpm / 60.;
            let phase = beat.fract() * 60. / bpm;
            let accent = if beat.floor() as usize % 4 == 0 {
                0.8
            } else {
                0.4
            };
            let s = accent * (-phase * 40.).exp() * (std::f32::consts::TAU * 70. * phase).sin();
            samples.extend([s, s]);
        }
        samples
    }
    #[test]
    fn silence_is_unknown_and_never_supplies_a_confident_grid() {
        let a = analyze(TrackId(1), &vec![0.; 44100 * 4 * 2], 44100);
        assert_eq!(a.sample_rate, 44100);
        assert_eq!(a.key, None);
        assert_eq!(a.key_confidence, 0.);
        assert_eq!(a.tempo.segments[0].confidence, 0.);
        assert!(a.tempo.beats.is_empty());
        assert!(a.tempo.downbeats.is_empty());
        assert!(
            a.bars
                .iter()
                .all(|b| b.rms == 0. && b.section == S::Silence)
        );
    }
    #[test]
    fn accented_click_grid_tracks_fractional_bpm_at_source_rates() {
        for sr in [44100, 48000] {
            let a = analyze(TrackId(1), &drums(sr, 127.5, 24.), sr);
            eprintln!(
                "sr={sr} bpm={} confidence={} first={:?}",
                a.tempo.global_bpm,
                a.tempo.segments[0].confidence,
                a.tempo.beats.first()
            );
            assert!((a.tempo.global_bpm - 127.5).abs() < 0.3);
            assert!(a.tempo.beats.windows(2).all(|p| p[1] > p[0]));
            assert!(
                a.bars
                    .iter()
                    .all(|b| b.rms.is_finite() && b.vocal_presence.is_finite())
            );
            assert!(a.bars.iter().any(|b| b.kick_salience > 0.3));
        }
    }
    #[test]
    fn analysis_finds_trailing_silence_and_sustained_foreground() {
        let sr = 22050;
        let mut samples = Vec::new();
        for i in 0..sr * 12 {
            let t = i as f32 / sr as f32;
            let s = if t < 8. {
                [220., 277.18, 329.63]
                    .iter()
                    .map(|hz| (std::f32::consts::TAU * hz * t).sin() * 0.1)
                    .sum::<f32>()
            } else {
                0.
            };
            samples.extend([s, s]);
        }
        let a = analyze(TrackId(2), &samples, sr);
        assert!(a.bars.iter().any(|b| b.vocal_presence > 0.5));
        assert!(a.bars.last().is_some_and(|b| b.section == S::Silence));
        assert!(a.key.is_some());
    }

    #[test]
    fn unaccented_beats_do_not_claim_reliable_downbeats() {
        let sr = 22050;
        let mut samples = drums(sr, 120., 32.);
        for (i, frame) in samples.chunks_exact_mut(2).enumerate() {
            if (i as f32 / sr as f32 * 2.).floor() as usize % 4 == 0 {
                frame[0] *= 0.5;
                frame[1] *= 0.5;
            }
        }
        let a = analyze(TrackId(1), &samples, sr);
        assert!((a.tempo.global_bpm - 120.).abs() < 0.3);
        assert!(a.tempo.segments.iter().all(|s| s.confidence < 0.65));
        let accented = analyze(TrackId(2), &drums(sr, 120., 32.), sr);
        assert!(accented.tempo.segments.iter().any(|s| s.confidence >= 0.65));
    }

    #[test]
    fn tempo_change_cannot_inherit_confidence_from_a_constant_region() {
        let sr = 22050;
        let mut samples = drums(sr, 120., 24.);
        samples.extend(drums(sr, 132., 24.));
        let a = analyze(TrackId(1), &samples, sr);
        assert!(a.tempo.segments.len() > 1);
        assert!(a.tempo.segments.iter().any(|s| s.confidence < 0.65));
    }

    #[test]
    fn tempo_segments_measure_distinct_section_bpms() {
        let sr = 22050;
        let mut samples = drums(sr, 100., 24.);
        samples.extend(drums(sr, 140., 24.));
        let a = analyze(TrackId(3), &samples, sr);
        let first = a.tempo.segments.first().unwrap().bpm;
        let last = a.tempo.segments.last().unwrap().bpm;
        assert!(first < 115., "first segment BPM was {first}");
        assert!(last > 125., "last segment BPM was {last}");
        assert!(a.tempo.beats.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn structure_classifier_exposes_dj_entry_and_exit_regions() {
        let mut samples = drums(22050, 120., 64.);
        for (i, frame) in samples.chunks_exact_mut(2).enumerate() {
            let sec = i as f32 / 22050.;
            if sec < 16. || sec >= 48. {
                frame[0] *= 0.3;
                frame[1] *= 0.3;
            }
        }
        let a = analyze(TrackId(4), &samples, 22050);
        assert!(a.sections.iter().any(|s| s.label == S::Intro));
        assert!(a.sections.iter().any(|s| s.label == S::Drop));
        assert!(a.sections.iter().any(|s| s.label == S::Outro));
        assert!(a.bars.windows(2).all(|w| w[1].start_sec >= w[0].start_sec));
    }

    #[test]
    fn antiphase_stereo_remains_audible_to_structure_detection() {
        let mut samples = drums(22050, 120., 12.);
        for frame in samples.chunks_exact_mut(2) {
            frame[1] = -frame[0];
        }
        let a = analyze(TrackId(5), &samples, 22050);
        assert!(
            a.bars
                .iter()
                .any(|b| b.rms > 0.01 && b.section != S::Silence)
        );
        assert!(!a.tempo.beats.is_empty());
    }
}
