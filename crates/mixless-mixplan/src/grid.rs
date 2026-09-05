use mixless_protocol::TrackAnalysis;

/// Beat timestamps win over the integrated tempo map; the latter also supports
/// partial analyses that have no explicit grid yet.
pub(crate) struct Grid<'a>(pub &'a TrackAnalysis);

impl Grid<'_> {
    pub fn meter(&self) -> f32 {
        self.0.tempo.meter_num.max(1) as f32
    }
    pub fn bpm(&self, beat: f32) -> f32 {
        let bpm = self.0.tempo.bpm_at_beat(beat);
        if bpm.is_finite() && bpm > 0.0 {
            bpm.clamp(20.0, 400.0)
        } else {
            120.0
        }
    }
    pub fn sec(&self, beat: f32) -> f32 {
        let beats = &self.0.tempo.beats;
        if beats.len() >= 2 {
            let i = (beat.max(0.0).floor() as usize).min(beats.len() - 2);
            return beats[i] + (beat - i as f32) * (beats[i + 1] - beats[i]);
        }
        let mut pos = 0.0;
        let mut sec = self.0.tempo.downbeats.first().copied().unwrap_or(0.0);
        // Integrate even maps with gaps rather than reading only global_bpm.
        for segment in &self.0.tempo.segments {
            let start = segment.start_beat.max(pos).min(beat);
            if start > pos {
                sec += (start - pos) * 60.0 / self.bpm(pos);
            }
            pos = start;
            let end = segment.end_beat.min(beat);
            if end > pos {
                sec += (end - pos) * 60.0 / self.bpm(pos);
                pos = end;
            }
            if pos >= beat {
                break;
            }
        }
        sec + (beat - pos) * 60.0 / self.bpm(pos)
    }
    pub fn beat(&self, sec: f32) -> f32 {
        let beats = &self.0.tempo.beats;
        if beats.len() >= 2 {
            let i = beats
                .partition_point(|t| *t <= sec)
                .saturating_sub(1)
                .min(beats.len() - 2);
            return (i as f32 + (sec - beats[i]) / (beats[i + 1] - beats[i])).max(0.0);
        }
        let mut lo = 0.0;
        let mut hi = self.0.duration_sec * 400.0 / 60.0 + 16.0;
        for _ in 0..32 {
            let mid = (lo + hi) * 0.5;
            if self.sec(mid) < sec {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) * 0.5
    }
    pub fn floor_bar(&self, sec: f32) -> f32 {
        let downbeats = &self.0.tempo.downbeats;
        if !downbeats.is_empty() {
            let i = downbeats
                .partition_point(|t| *t <= sec + 0.0001)
                .saturating_sub(1);
            return self.beat(downbeats[i]);
        }
        (self.beat(sec) / self.meter() + 0.00001).floor() * self.meter()
    }
    pub fn ceil_bar(&self, sec: f32) -> f32 {
        let floor = self.floor_bar(sec);
        if self.sec(floor) + 0.0001 >= sec {
            floor
        } else {
            floor + self.meter()
        }
    }
}
