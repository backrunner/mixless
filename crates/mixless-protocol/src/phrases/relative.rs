//! Local arrangement contrast inside coarse peak labels. This is an overlap
//! opportunity, not a statement that the voice or a held melody has finished.
use crate::{SectionLabel as S, TrackAnalysis};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};

type Windows = Vec<(f32, f32)>;
struct Cache([(usize, Windows); 2]);
thread_local! { static CACHE: RefCell<Option<Arc<Cache>>> = const { RefCell::new(None) }; }

/// Reuse measured evidence while immutable analyses are borrowed by a planner.
/// No result survives a call, and nested calls restore their parent's cache.
/// The scope stays on its creating thread, like the thread-local cache itself.
pub struct PeakScope<'a> {
    previous: Option<Arc<Cache>>,
    borrowed: PhantomData<(&'a TrackAnalysis, Rc<()>)>,
}
/// Immutable prepared evidence can be shared across workers; its input borrows
/// prevent pointer reuse or reanalysis while any worker can install it.
pub struct PeakEvidence<'a> {
    values: Arc<Cache>,
    borrowed: PhantomData<&'a TrackAnalysis>,
}
impl<'a> PeakEvidence<'a> {
    pub fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self {
            values: Arc::new(Cache(
                [a, b].map(|t| (t as *const _ as usize, relative_recoveries(t))),
            )),
            borrowed: PhantomData,
        }
    }
    pub fn enter(&self) -> PeakScope<'a> {
        PeakScope {
            previous: CACHE.with(|c| c.replace(Some(Arc::clone(&self.values)))),
            borrowed: PhantomData,
        }
    }
}
impl<'a> PeakScope<'a> {
    pub fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        PeakEvidence::new(a, b).enter()
    }
}
impl Drop for PeakScope<'_> {
    fn drop(&mut self) {
        CACHE.with(|c| c.replace(self.previous.take()));
    }
}

/// Sustained, two-sided arrangement retreat with a surviving rhythmic or tonal
/// bed. A limited mix can hide a kit withdrawal; a continuing kit can instead
/// carry a foreground withdrawal supported by the mixture's level contour.
/// Missing stems/clock or ambiguous local contrast yields no new permission.
pub fn relative_recoveries(t: &TrackAnalysis) -> Windows {
    if let Some(w) = CACHE.with(|c| {
        c.borrow().as_ref().and_then(|c| {
            c.0.iter()
                .find(|(key, _)| *key == t as *const _ as usize)
                .map(|(_, w)| w.clone())
        })
    }) {
        return w;
    }
    detect(t)
}

#[derive(Clone, Copy)]
struct Cell {
    start: f32,
    end: f32,
    power: [f32; 3],
    attacks: f32,
    mix: f32,
    melody: f32,
}
impl Cell {
    fn kit_db(self) -> f32 {
        10. * self.power[1].max(f32::MIN_POSITIVE).log10()
    }
}

fn profile(t: &TrackAnalysis) -> Option<Vec<Cell>> {
    let stems = t.stems.as_ref()?;
    if t.bars
        .iter()
        .any(|b| !b.start_sec.is_finite() || !b.end_sec.is_finite() || b.end_sec <= b.start_sec)
        || t.bars
            .windows(2)
            .any(|b| (b[0].end_sec - b[1].start_sec).abs() > 0.01)
    {
        return None;
    }
    let mut result = Vec::with_capacity(t.bars.len());
    for b in &t.bars {
        let first = stems.frames.partition_point(|f| f.end_sec <= b.start_sec);
        let (mut seconds, mut power, mut attacks, mut melody) = (0., [0.; 3], 0., 0.);
        if !b.rms.is_finite() || b.rms < 0. {
            return None;
        }
        let mut covered_until = b.start_sec;
        for (index, f) in stems.frames[first..]
            .iter()
            .take_while(|f| f.start_sec < b.end_sec)
            .enumerate()
        {
            // Reject holes, overlaps and nonfinite/inverted measurements.
            if f.start_sec > covered_until + 0.01
                || (index > 0 && f.start_sec < covered_until - 0.01)
                || f.end_sec <= f.start_sec
                || !f.onset[1].is_finite()
                || f.onset[1] < 0.
                || f.rms.iter().any(|v| !v.is_finite() || *v < 0.)
                || f.band_db[2][1..].iter().any(|v| !v.is_finite())
            {
                return None;
            }
            let w = (f.end_sec.min(b.end_sec) - f.start_sec.max(b.start_sec)).max(0.);
            seconds += w;
            for i in 0..3 {
                power[i] += f.rms[i].powi(2) * w;
            }
            attacks += f.onset[1] * w;
            melody += f.band_db[2][1..]
                .iter()
                .map(|db| 10f32.powf(db / 10.))
                .sum::<f32>()
                * w;
            covered_until = f.end_sec;
        }
        if seconds < (b.end_sec - b.start_sec) * 0.95 || seconds <= 0. {
            return None;
        }
        result.push(Cell {
            start: b.start_sec,
            end: b.end_sec,
            power: power.map(|p| p / seconds),
            attacks: attacks / seconds,
            mix: b.rms.powi(2),
            melody: melody / seconds,
        });
    }
    Some(result)
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.)
}
fn mean(cells: &[Cell], f: impl Fn(Cell) -> f32) -> f32 {
    let seconds: f32 = cells.iter().map(|c| c.end - c.start).sum();
    cells.iter().map(|&c| f(c) * (c.end - c.start)).sum::<f32>() / seconds
}
fn contrast(before: &[Cell], gap: &[Cell], after: &[Cell]) -> bool {
    kit_retreat(before, gap, after) || foreground_retreat(before, gap, after)
}

fn kit_retreat(before: &[Cell], gap: &[Cell], after: &[Cell]) -> bool {
    // Each side must independently support the contrast, beyond its ordinary
    // bar-to-bar variation. Two independent dimensions distinguish a partial
    // kit withdrawal from a drum fader change, bass mute or a different sound.
    [before, after].into_iter().all(|side| {
        let mut levels: Vec<_> = side.iter().map(|c| c.kit_db()).collect();
        levels.sort_by(f32::total_cmp);
        let floor = levels[(levels.len() - 1) / 4];
        let db = median(levels);
        let mad = median(side.iter().map(|c| (c.kit_db() - db).abs()).collect());
        let attack = mean(side, |c| c.attacks);
        let noise = median(side.iter().map(|c| (c.attacks - attack).abs()).collect());
        let kit = mean(side, |c| c.power[1]);
        let tonal = mean(side, |c| c.power[2]);
        kit > mean(side, |c| c.power.iter().sum::<f32>()) * 0.01
            && floor - 10. * mean(gap, |c| c.power[1]).max(f32::MIN_POSITIVE).log10()
                >= 3_f32.max(2. * mad)
            && attack > 0.
            && attack - mean(gap, |c| c.attacks) >= (0.1 * attack).max(2. * noise)
            && gap.iter().all(|c| c.kit_db() <= db - 2.)
            && mean(gap, |c| c.power[2]) >= tonal * 0.5
    }) && gap
        .iter()
        .all(|c| c.power[2] > c.power.iter().sum::<f32>() * 0.1)
}

/// A kit need not stop for the arrangement to breathe. Require a sustained
/// retreat of BOTH non-percussive foreground power and instrument mids/highs,
/// corroborated by the mix contour on each side. A bass mute, a vocal rest or
/// a drum fader change alone therefore cannot split the protected peak.
fn foreground_retreat(before: &[Cell], gap: &[Cell], after: &[Cell]) -> bool {
    let db = |p: f32| 10. * p.max(f32::MIN_POSITIVE).log10();
    let retreat = |side: &[Cell], measure: fn(Cell) -> f32, minimum: f32| {
        let levels: Vec<_> = side.iter().map(|&c| db(measure(c))).collect();
        let centre = median(levels.clone());
        let mad = median(levels.iter().map(|x| (x - centre).abs()).collect());
        let mut levels = levels;
        levels.sort_by(f32::total_cmp);
        levels[(levels.len() - 1) / 4] - db(mean(gap, measure)) >= minimum.max(2. * mad)
            && gap
                .iter()
                .all(|&c| db(measure(c)) <= centre - minimum * (2. / 3.))
    };
    let foreground = |c: Cell| c.power[0] + c.power[2];
    [before, after].into_iter().all(|side| {
        // 3 dB = half foreground power. A smaller 1.5 dB mix contour is
        // expected while the unchanged kit continues contributing energy.
        retreat(side, foreground, 3.)
            && retreat(side, |c| c.melody, 3.)
            && retreat(side, |c| c.mix, 1.5)
            && mean(gap, |c| c.power[1]) >= mean(side, |c| c.power[1]) * 0.5
            && mean(gap, |c| c.attacks) >= mean(side, |c| c.attacks) * 0.5
    }) && gap
        .iter()
        .all(|c| c.power[1] > c.power.iter().sum::<f32>() * 0.1 && c.attacks > 0.)
}

fn clock_supported(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let first = t.tempo.beats.partition_point(|b| *b < start - 0.01);
    let last = t.tempo.beats.partition_point(|b| *b < end - 0.01);
    first < last
        && last < t.tempo.beats.len()
        && (first..last).all(|i| {
            t.tempo.segments.iter().enumerate().any(|(j, s)| {
                i as f32 >= s.start_beat
                    && (i as f32) < s.end_beat
                    && t.tempo
                        .pulse_confidence
                        .get(j)
                        .copied()
                        .unwrap_or(s.confidence)
                        >= 0.65
            })
        })
}

fn detect(t: &TrackAnalysis) -> Windows {
    if t.tempo.beats.len() < 8 || t.tempo.downbeats.len() < 2 {
        return vec![];
    }
    let Some(cells) = profile(t) else {
        return vec![];
    };
    // Phrase-scale evidence, rather than a fixed number of seconds. The
    // median spacing of the analysis' periodic phrase lattice is unchanged
    // by a half/double clock. Fall back to four bars of a normalized pulse
    // when that independent lattice is absent.
    let lattice: Vec<_> = t
        .phrase_boundaries
        .windows(2)
        .filter(|p| p[0].confidence >= 0.5 && p[1].confidence >= 0.5 && p[1].novelty < 0.15)
        .map(|p| p[1].time_sec - p[0].time_sec)
        .filter(|d| *d > 0.)
        .collect();
    let phrase_half = (lattice.len() >= 3).then(|| median(lattice) * 0.5);
    let mut result: Windows = vec![];
    for i in 1..cells.len() {
        let start = cells[i].start;
        let beat = t.tempo.beats.partition_point(|b| *b < start - 0.01) as f32;
        let bpm = t.tempo.bpm_at_beat(beat);
        if !bpm.is_finite() || bpm <= 0. {
            continue;
        }
        let mut pulse = bpm;
        while pulse < 80. {
            pulse *= 2.;
        }
        while pulse >= 160. {
            pulse *= 0.5;
        }
        let minimum = phrase_half.unwrap_or(4. * t.tempo.meter_num.max(1) as f32 * 60. / pulse);
        for j in i + 1..cells.len() {
            let end = cells[j].start;
            let width = end - start;
            if width > minimum * 4. + 0.05 {
                break;
            }
            if width < minimum - 0.05 || j - i < 2 {
                continue;
            }
            // Remain strictly within a contiguous coarse peak on both sides.
            // At least a complete phrase precedes the release and follows its
            // return; one impact or a terminal fade cannot manufacture a peak.
            let lo = cells.partition_point(|c| c.end <= start - width + 0.05);
            let hi = cells.partition_point(|c| c.start < end + width - 0.05);
            if lo >= i
                || hi <= j
                || cells[lo].start > start - width + 0.1
                || cells[hi - 1].end < end + width - 0.1
            {
                continue;
            }
            let span_start = cells[lo].start;
            let span_end = cells[hi - 1].end;
            let sections: Vec<_> = t
                .sections
                .iter()
                .filter(|s| s.start_sec < span_end && s.end_sec > span_start)
                .collect();
            if sections.is_empty()
                || sections
                    .iter()
                    .any(|s| !matches!(s.label, S::Drop | S::Chorus))
                || sections.first().unwrap().start_sec > span_start + 0.05
                || sections.last().unwrap().end_sec < span_end - 0.05
                || sections
                    .windows(2)
                    .any(|s| (s[0].end_sec - s[1].start_sec).abs() > 0.05)
                || !clock_supported(t, start, end)
                || crate::has_buildup(&t.bars, start, end)
            {
                continue;
            }
            if contrast(&cells[lo..i], &cells[i..j], &cells[j..hi]) {
                // Keep complete measured intervals, never union disjoint
                // candidates into an unmeasured larger interval.
                if let Some(last) = result.last_mut().filter(|w| start < w.1) {
                    if width > last.1 - last.0 {
                        *last = (start, end);
                    }
                } else {
                    result.push((start, end));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod foreground_tests;
