use serde::{Deserialize, Serialize};

/// Legacy instrument controls included bass. Preserve that meaning when reading
/// three-lane snapshots/plans; new output always contains four independent lanes.
pub(crate) fn four_lanes<'de, D, T>(deserializer: D) -> Result<[T; 4], D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Clone,
{
    let mut values = Vec::<T>::deserialize(deserializer)?;
    if values.len() == 3 {
        values.push(values[2].clone());
    }
    values
        .try_into()
        .map_err(|_| serde::de::Error::custom("expected three legacy or four stem lanes"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemKind {
    Vocals,
    Drums,
    Instruments,
    Bass,
}
impl StemKind {
    pub fn index(self) -> usize {
        match self {
            Self::Vocals => 0,
            Self::Drums => 1,
            Self::Instruments => 2,
            Self::Bass => 3,
        }
    }
}

#[cfg(test)]
mod compatibility_tests {
    #[test]
    fn legacy_plan_instrument_lane_also_controls_bass() {
        let line = |v| serde_json::json!({"nodes":[[0.0,v]]});
        let old = serde_json::json!({"outgoing":[line(1.),line(0.5),line(0.2)],
            "incoming":[line(0.),line(1.),line(0.8)]});
        let mix: crate::StemMix = serde_json::from_value(old).unwrap();
        assert_eq!(mix.outgoing[3].sample(0.), 0.2);
        assert_eq!(mix.incoming[3].sample(0.), 0.8);
        assert_eq!(
            serde_json::to_value(mix).unwrap()["outgoing"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
}

/// Model estimates in original source seconds; not a ground-truth score.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StemNote {
    pub stem: StemKind,
    pub start_sec: f32,
    pub end_sec: f32,
    pub midi: u8,
    pub confidence: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StemFrame {
    pub start_sec: f32,
    pub end_sec: f32,
    /// In vocals / drums / instruments order. No independent normalization.
    pub rms: [f32; 3],
    pub band_db: [[f32; 3]; 3],
    pub onset: [f32; 3],
    pub drum_low_onset: f32,
    pub vocal_activity: f32,
    pub note_chroma: [f32; 12],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StemAnalysis {
    pub version: u32,
    pub separator_sha256: String,
    pub notes_sha256: String,
    pub duration_sec: f32,
    pub residual_rms: f32,
    pub frames: Vec<StemFrame>,
    pub notes: Vec<StemNote>,
}
impl StemAnalysis {
    pub fn frame_at(&self, sec: f32) -> Option<&StemFrame> {
        self.frames
            .get(self.frames.partition_point(|f| f.end_sec <= sec))
            .filter(|f| f.start_sec <= sec)
    }
    pub fn note_crossing(&self, sec: f32, stem: StemKind) -> bool {
        self.notes[..self.notes.partition_point(|n| n.start_sec < sec - 0.04)]
            .iter()
            .any(|n| n.stem == stem && n.end_sec > sec + 0.04 && n.confidence >= 0.45)
    }
    pub fn valid(&self, duration: f32) -> bool {
        duration.is_finite()
            && duration > 0.
            && (duration - self.duration_sec).abs() < 0.05
            && self.residual_rms.is_finite()
            && self.residual_rms >= 0.
            && !self.frames.is_empty()
            && self.frames[0].start_sec.abs() < 0.001
            && (self.frames.last().unwrap().end_sec - duration).abs() < 0.05
            && self
                .frames
                .windows(2)
                .all(|f| (f[0].end_sec - f[1].start_sec).abs() < 0.001)
            && self.frames.iter().all(|f| {
                f.start_sec.is_finite()
                    && f.end_sec.is_finite()
                    && f.start_sec >= 0.
                    && f.end_sec > f.start_sec
                    && f.rms.iter().all(|v| v.is_finite() && *v >= 0.)
                    && f.band_db.iter().flatten().all(|v| v.is_finite())
                    && f.onset
                        .iter()
                        .chain(&f.note_chroma)
                        .chain([&f.drum_low_onset, &f.vocal_activity])
                        .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            })
            && self
                .notes
                .windows(2)
                .all(|n| n[0].start_sec <= n[1].start_sec)
            && self.notes.iter().all(|n| {
                n.start_sec.is_finite()
                    && n.end_sec.is_finite()
                    && n.start_sec >= 0.
                    && n.end_sec > n.start_sec
                    && n.end_sec <= duration + 0.02
                    && n.midi <= 127
                    && n.confidence.is_finite()
                    && (0. ..=1.).contains(&n.confidence)
            })
    }
}
