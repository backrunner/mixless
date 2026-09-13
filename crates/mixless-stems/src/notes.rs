// Basic Pitch model I/O: Spotify Basic Pitch v0.4.0, Apache-2.0.
// Native event decoding below uses explicit sample times rather than assuming
// independently cropped model windows form a perfectly uniform frame grid.
use crate::{check, Error, Inference, Progress, Result};
use ort::value::Tensor;
use serde::{Deserialize, Serialize};

const SAMPLES: usize = 43844;
const HOP: usize = 256;
const TRIM: usize = 15;
const STRIDE: usize = SAMPLES - 2 * TRIM * HOP;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub start_sec: f32,
    pub end_sec: f32,
    pub midi: u8,
    pub confidence: f32,
}

impl Inference {
    pub fn transcribe(
        &mut self,
        stereo: &[f32],
        stem: &'static str,
        progress: &mut impl FnMut(Progress),
        active: &impl Fn() -> bool,
    ) -> Result<Vec<Note>> {
        let mono: Vec<_> = stereo
            .chunks_exact(2)
            .map(|s| (s[0] + s[1]) * 0.5)
            .collect();
        let mono = crate::resample::convert(&mono, 1, 44100, 22050);
        let mut frames = Vec::new();
        let mut times = Vec::new();
        for start in (0..mono.len() + TRIM * HOP).step_by(STRIDE) {
            check(active)?;
            progress(Progress::Notes {
                stem,
                percent: (100 * start / (mono.len() + TRIM * HOP).max(1)).min(100) as u8,
            });
            let mut audio = vec![0.; SAMPLES];
            for (i, sample) in audio.iter_mut().enumerate() {
                let at = start as isize + i as isize - (TRIM * HOP) as isize;
                if at >= 0 && (at as usize) < mono.len() {
                    *sample = mono[at as usize];
                }
            }
            let out = self.notes.run(ort::inputs!["serving_default_input_2:0" => Tensor::from_array(([1usize,SAMPLES,1],audio))?])?;
            let (shape, note) = out["StatefulPartitionedCall:1"].try_extract_tensor::<f32>()?;
            let (oshape, onset) = out["StatefulPartitionedCall:2"].try_extract_tensor::<f32>()?;
            if shape.len() != 3
                || shape[0] != 1
                || shape[2] != 88
                || shape != oshape
                || shape[1] <= (2 * TRIM) as i64
            {
                return Err(Error::Model("Unexpected note model output shape".into()));
            }
            if note.iter().chain(onset).any(|v| !v.is_finite()) {
                return Err(Error::Model("Non-finite note evidence".into()));
            }
            for f in TRIM..shape[1] as usize - TRIM {
                let sec = (start + (f - TRIM) * HOP) as f32 / 22050.;
                if sec >= mono.len() as f32 / 22050. {
                    break;
                }
                if times.last().is_some_and(|t| *t >= sec) {
                    continue;
                }
                times.push(sec);
                frames.push(std::array::from_fn::<_, 88, _>(|p| {
                    (
                        note[f * 88 + p].clamp(0., 1.),
                        onset[f * 88 + p].clamp(0., 1.),
                    )
                }));
            }
        }
        Ok(decode(&times, &frames, mono.len() as f32 / 22050.))
    }
}

fn decode(times: &[f32], frames: &[[(f32, f32); 88]], duration: f32) -> Vec<Note> {
    let mut notes = Vec::new();
    for pitch in 0..88 {
        let mut start = None;
        let mut last = 0;
        let mut sum = 0.;
        let mut count = 0;
        let mut previous = 0.;
        let finish = |start: usize, last: usize, sum: f32, count: usize, notes: &mut Vec<Note>| {
            let end = (times[last] + HOP as f32 / 22050.).min(duration);
            if end - times[start] >= 0.09 && count > 0 {
                notes.push(Note {
                    start_sec: times[start],
                    end_sec: end,
                    midi: pitch as u8 + 21,
                    confidence: (sum / count as f32).clamp(0., 1.),
                });
            }
        };
        for (i, frame) in frames.iter().enumerate() {
            let (energy, onset) = frame[pitch];
            let attack = onset >= 0.5
                && onset > previous
                && (i + 1 == frames.len() || onset >= frames[i + 1][pitch].1);
            if let Some(s) = start {
                if (attack && times[i] - times[s] > 0.09) || times[i] - times[last] > 0.075 {
                    finish(s, last, sum, count, &mut notes);
                    start = None;
                    sum = 0.;
                    count = 0;
                }
            }
            if start.is_none() && (attack || energy >= 0.5) {
                start = Some(i);
                last = i;
            }
            if start.is_some() && energy >= 0.3 {
                last = i;
                sum += energy;
                count += 1;
            }
            previous = onset;
        }
        if let Some(s) = start {
            finish(s, last, sum, count, &mut notes);
        }
    }
    notes.sort_by(|a, b| {
        a.start_sec
            .total_cmp(&b.start_sec)
            .then(a.midi.cmp(&b.midi))
    });
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_notes_survive_chunk_boundaries_but_repeated_attacks_split_them() {
        let times: Vec<_> = (0..300).map(|i| i as f32 * 256. / 22050.).collect();
        let mut frames = vec![[(0., 0.); 88]; 300];
        for f in &mut frames[10..250] {
            f[39] = (0.8, 0.);
        }
        frames[10][39].1 = 0.9;
        frames[180][39].1 = 0.9;
        let notes = decode(&times, &frames, 4.);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].midi, 60);
        assert!((notes[0].start_sec - times[10]).abs() < 1e-6);
        assert!(notes[0].end_sec > 2. && notes[1].start_sec > 2.);
    }
}
