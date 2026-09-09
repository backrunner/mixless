use crate::{grid::Grid, Candidate};
use mixless_protocol::{BarMap, Cue, CueKind, SectionLabel as S, StrategyId as Id, TrackAnalysis};

#[derive(Debug, Clone, Copy, Default)]
pub struct ScoreComponents {
    pub phrase_align: f32,
    pub section_compat: f32,
    pub key_compat: f32,
    pub tempo_compat: f32,
    pub energy_continuity: f32,
    pub vocal_clash: f32,
    pub kick_compat: f32,
    pub stretch_penalty: f32,
    pub length_mismatch: f32,
}

impl ScoreComponents {
    pub fn normalized(self) -> f32 {
        let good = [
            self.phrase_align,
            self.section_compat,
            self.key_compat,
            self.tempo_compat,
            self.energy_continuity,
            1.0 - self.vocal_clash,
            self.kick_compat,
            1.0 - self.stretch_penalty,
            1.0 - self.length_mismatch,
        ];
        good.into_iter()
            .zip([1.2, 1.4, 1.0, 1.0, 0.8, 1.3, 0.7, 0.9, 0.4])
            .map(|(x, w)| {
                if x.is_finite() {
                    x.clamp(0.0, 1.0) * w
                } else {
                    0.0
                }
            })
            .sum::<f32>()
            / 8.7
    }
}

// Pitch classes indexed by Camelot number (1..12), minor then major.
const MINOR: [i32; 12] = [8, 3, 10, 5, 0, 7, 2, 9, 4, 11, 6, 1];
const MAJOR: [i32; 12] = [11, 6, 1, 8, 3, 10, 5, 0, 7, 2, 9, 4];
fn sounding_camelot(track: &TrackAnalysis, pitch: f32) -> Option<(i32, bool)> {
    let cam = track.camelot.as_deref()?;
    let minor = cam.ends_with('A');
    if !minor && !cam.ends_with('B') {
        return None;
    }
    let number: usize = cam.get(..cam.len().checked_sub(1)?)?.parse().ok()?;
    if !(1..=12).contains(&number) || (pitch - pitch.round()).abs() > 0.05 {
        return None;
    }
    let table = if minor { MINOR } else { MAJOR };
    let pc = (table[number - 1] + pitch.round() as i32).rem_euclid(12);
    Some((table.iter().position(|v| *v == pc)? as i32, minor))
}
fn compatibility(a: (i32, bool), b: (i32, bool)) -> f32 {
    if a == b {
        1.0
    } else if a.0 == b.0 || (a.1 == b.1 && matches!((a.0 - b.0).rem_euclid(12), 1 | 11)) {
        0.85
    } else {
        0.0
    }
}

pub(crate) fn key_match(a: &TrackAnalysis, b: &TrackAnalysis, pa: f32, pb: f32) -> (f32, f32) {
    let Some(ka) = sounding_camelot(a, pa) else {
        return (0.0, 0.0);
    };
    let Some(kb) = sounding_camelot(b, pb) else {
        return (0.0, 0.0);
    };
    let compatible = compatibility(ka, kb);
    if compatible > 0.0 {
        return (compatible, 0.0);
    }
    for shift in [-1.0, 1.0, -2.0, 2.0] {
        if sounding_camelot(b, pb + shift).is_some_and(|k| compatibility(ka, k) > 0.0) {
            return (if shift.abs() == 1.0 { 0.5 } else { 0.25 }, shift);
        }
    }
    (0.0, 0.0)
}

pub(crate) fn section(track: &TrackAnalysis, sec: f32) -> S {
    track
        .sections
        .iter()
        .find(|s| sec >= s.start_sec && sec < s.end_sec)
        .map(|s| s.label)
        .or_else(|| {
            track
                .bars
                .iter()
                .find(|b| sec >= b.start_sec && sec < b.end_sec)
                .map(|b| b.section)
        })
        .unwrap_or(S::Unknown)
}
fn features(track: &TrackAnalysis, sec: f32) -> (f32, f32, f32, f32) {
    track
        .bars
        .iter()
        .find(|b| sec >= b.start_sec && sec < b.end_sec)
        .map(|b| (b.rms, b.kick_salience, b.vocal_presence, b.energy_slope))
        .unwrap_or((0.0, 0.0, 0.0, 0.0))
}

pub(crate) fn vocal_forbidden(c: &Candidate, a: &TrackAnalysis, b: &TrackAnalysis) -> bool {
    let samples = (c.n as usize * 4).max(1);
    (0..samples).any(|j| {
        let t = (j as f32 + 0.5) / samples as f32;
        let ta = c.start_sec + (c.out_sec - c.start_sec) * t;
        let tb = c.in_sec + (c.end_sec - c.in_sec) * t;
        features(a, ta).2 > 0.5
            && features(b, tb).2 > 0.5
            && section(a, ta) == S::Verse
            && section(b, tb) == S::Chorus
    })
}

pub(crate) fn score(
    c: &Candidate,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    ca: &[Cue],
    cb: &[Cue],
) -> Option<f32> {
    let ga = Grid(a);
    let gb = Grid(b);
    let sa = section(a, c.out_sec - 0.001);
    let sb = section(b, c.in_sec + 0.001);
    let mut clashes = 0.0;
    let mut forbidden = false;
    let samples = (c.n as usize * 4).max(1);
    for j in 0..samples {
        let t = (j as f32 + 0.5) / samples as f32;
        let ta = c.start_sec + (c.out_sec - c.start_sec) * t;
        let tb = c.in_sec + (c.end_sec - c.in_sec) * t;
        if features(a, ta).2 > 0.5 && features(b, tb).2 > 0.5 {
            clashes += 1.0;
            forbidden |= section(a, ta) == S::Verse && section(b, tb) == S::Chorus;
        }
    }
    if forbidden {
        return None;
    }
    let structure = match (sa, sb) {
        (S::Unknown, _) | (_, S::Unknown) => return None,
        (S::Drop | S::Chorus, S::Intro)
        | (S::BuildUp, S::Drop | S::Chorus)
        | (S::Outro, S::Intro) => 1.0,
        (S::Break | S::Breakdown, S::Intro | S::Break | S::Breakdown) => 0.75,
        (S::Chorus, S::Drop | S::Chorus) => 0.55,
        (S::Drop, S::Drop) => {
            if c.strategy != Id::DropCut {
                return None;
            }
            0.25
        }
        _ => 0.25,
    };
    // Catalog eligibility also gives deterministic preference to musically useful
    // strategies rather than selecting the first of eight identical scores.
    let kicks = (features(a, c.out_sec - 0.01).1 + features(b, c.in_sec + 0.01).1) * 0.5;
    let eligible = match c.strategy {
        Id::DryCut => {
            features(a, c.out_sec - 0.01).1 >= 0.55
                && features(b, c.in_sec + 0.01).1 >= 0.55
                && features(a, c.out_sec - 0.01).2 < 0.35
                && features(b, c.in_sec + 0.01).2 < 0.65
        }
        Id::BassSwap => kicks >= 0.5 || matches!(sa, S::Break | S::Breakdown | S::Chorus),
        Id::DropCut => matches!((sa, sb), (S::BuildUp | S::Drop, S::Drop | S::Chorus)),
        Id::EchoOut => sa == S::Outro,
        Id::LoopConstruct => sb == S::Intro && features(b, c.in_sec + 0.01).1 < 0.4,
        Id::ScratchCut => {
            let hot_in = cb.iter().any(|cue| {
                cue.user_set
                    && cue.kind == CueKind::Hot
                    && (cue.frame as f32 / b.sample_rate as f32 - c.in_sec).abs()
                        <= 60. / gb.bpm(c.in_beat)
            });
            c.n <= 8
                && a.tempo.beats.len() >= 8
                && b.tempo.beats.len() >= 8
                && a.tempo.downbeats.len() >= 2
                && b.tempo.downbeats.len() >= 2
                && a.tempo.segments.iter().any(|s| s.confidence >= 0.65)
                && b.tempo.segments.iter().any(|s| s.confidence >= 0.65)
                && features(a, c.out_sec - 0.01).1 >= 0.55
                && features(b, c.in_sec + 0.01).1 >= 0.55
                && features(a, c.out_sec - 0.01).2 < 0.35
                && features(b, c.in_sec + 0.01).2 < 0.65
                && hot_in
        }
        Id::EnergyHold => (c.ratio - 1.0).abs() > 0.08,
        Id::PhraseBlend | Id::BreakToIntro => true,
        Id::FilterSweep => c.key_score < 0.85,
        Id::FallbackSwapFilter => false,
    };
    if !eligible {
        return None;
    }
    let mut ea = 0.0;
    let mut eb = 0.0;
    for j in 0..4 {
        ea += features(a, ga.sec(c.out_beat - (j as f32 + 0.5) * ga.meter())).0;
        eb += features(b, gb.sec(c.in_beat + (j as f32 + 0.5) * gb.meter())).0;
    }
    let energy = if ea.max(eb) > 0.0 {
        1.0 - (ea - eb).abs() / ea.max(eb)
    } else {
        0.0
    };
    let hot = |cues: &[Cue], sr: u32, sec: f32, bar_sec: f32| {
        cues.iter().any(|c| {
            c.user_set
                && c.kind == CueKind::Hot
                && (c.frame as f32 / sr as f32 - sec).abs() <= 2.0 * bar_sec
        })
    };
    let phrase: f32 = if a.tempo.beats.is_empty() || b.tempo.beats.is_empty() {
        0.7
    } else {
        1.0
    };
    let hot_bonus = if hot(
        ca,
        a.sample_rate,
        c.start_sec,
        ga.meter() * 60.0 / ga.bpm(c.out_beat),
    ) || hot(
        cb,
        b.sample_rate,
        c.in_sec,
        gb.meter() * 60.0 / gb.bpm(c.in_beat),
    ) {
        0.05
    } else {
        0.0
    };
    let mapped = c.bar_map != BarMap::OneToOne;
    let diff = (c.ratio - 1.0).abs();
    let tempo = if mapped {
        1.0
    } else if diff <= 0.08 {
        1.0 - 7.5 * diff
    } else if diff <= 0.16 {
        0.4 - (diff - 0.08) * 3.75
    } else {
        0.1
    };
    let stretch = if c.literal {
        1.0
    } else if mapped {
        0.0
    } else {
        (((c.rate_a / c.offset_a.rate - 1.0)
            .abs()
            .max((c.rate_b / c.offset_b.rate - 1.0).abs())
            - 0.03)
            / 0.13)
            .clamp(0.0, 1.0)
    };
    let result = ScoreComponents {
        phrase_align: (phrase + hot_bonus).min(1.0),
        section_compat: structure,
        key_compat: c.key_score,
        tempo_compat: tempo,
        energy_continuity: energy,
        vocal_clash: clashes / samples as f32,
        kick_compat: (1.0
            - 1.5 * (features(a, c.out_sec - 0.01).1 - features(b, c.in_sec + 0.01).1).abs())
        .max(0.0),
        stretch_penalty: stretch,
        length_mismatch: (c.n as f32 - c.strategy.default_bars() as f32).abs()
            / c.strategy.default_bars() as f32,
    }
    .normalized();
    if sa == S::Drop && sb == S::Drop && result < 0.75 {
        None
    } else {
        Some(result)
    }
}
