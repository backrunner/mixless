//! Stable disk keys and cheap in-process invalidation. Fingerprinting runs on
//! the ordering worker, never in UI polling or on the audio thread.
use super::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp {
    content: u64,
    readiness: u64,
    live_moves: mixless_protocol::LiveMoves,
}

impl Stamp {
    pub(super) fn current(core: &AppCore) -> Self {
        Self {
            content: core.analysis.content_revision.load(Ordering::Acquire),
            readiness: core.analysis.revision.load(Ordering::Acquire),
            live_moves: core.settings.get().live_moves,
        }
    }
}

pub(super) struct Arranged {
    pub input: Vec<TrackId>,
    pub order: Vec<TrackId>,
    pub stamp: Option<Stamp>,
}

struct Fingerprint(Sha256);
impl std::io::Write for Fingerprint {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn key(
    tracks: &[crate::analysis::PreparedTrack],
    options: &mixless_mixplan::PlannerOptions,
) -> Result<String, serde_json::Error> {
    fingerprint(
        tracks.iter().map(|p| {
            (
                p.track.id,
                p.track.content_hash.as_str(),
                p.analysis.as_ref(),
            )
        }),
        options,
        mixless_analyze::ANALYSIS_VERSION,
        mixless_mixplan::order::ORDER_VERSION,
    )
}

fn fingerprint<'a>(
    tracks: impl Iterator<Item = (TrackId, &'a str, &'a mixless_protocol::TrackAnalysis)>,
    options: &mixless_mixplan::PlannerOptions,
    analysis_version: u32,
    order_version: u32,
) -> Result<String, serde_json::Error> {
    let mut sink = Fingerprint(Sha256::new());
    // Exhaustive destructuring makes new planner options a compile-time prompt
    // to extend the persistent key.
    let mixless_mixplan::PlannerOptions {
        harmonic_key_shift,
        smooth,
        who_stretches,
        literal_half_double,
        strategy,
        earliest_outgoing_sec,
        outgoing_entry_sec,
        stem_playback,
        live_moves,
    } = options;
    serde_json::to_writer(
        &mut sink,
        &(
            analysis_version,
            order_version,
            mixless_stems::VERSION,
            harmonic_key_shift,
            smooth,
            who_stretches,
            literal_half_double,
            strategy,
            earliest_outgoing_sec,
            outgoing_entry_sec,
            stem_playback,
            live_moves,
        ),
    )?;
    for track in tracks {
        serde_json::to_writer(&mut sink, &track)?;
    }
    Ok(format!("{:x}", sink.0.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::TrackAnalysis;

    #[test]
    fn keys_cover_sequence_occurrences_evidence_versions_and_options() {
        let mut analysis = TrackAnalysis {
            track_id: TrackId(1),
            duration_sec: 120.,
            sample_rate: 44100,
            tempo: Default::default(),
            key: None,
            camelot: None,
            key_confidence: 0.,
            sections: vec![],
            phrase_boundaries: vec![],
            mix_regions: vec![],
            bars: vec![],
            moments: vec![],
            stems: None,
            waveform_path: None,
            partial: false,
        };
        let options = mixless_mixplan::PlannerOptions::default();
        let hash = |ids: &[i64],
                    a: &TrackAnalysis,
                    o: &mixless_mixplan::PlannerOptions,
                    av,
                    ov,
                    source| {
            fingerprint(ids.iter().map(|id| (TrackId(*id), source, a)), o, av, ov).unwrap()
        };
        let original = hash(&[1, 2, 1], &analysis, &options, 1, 1, "audio");
        assert_eq!(
            original,
            hash(&[1, 2, 1], &analysis, &options, 1, 1, "audio")
        );
        for ids in [&[1, 1, 2][..], &[1, 2][..], &[1, 2, 3][..]] {
            assert_ne!(original, hash(ids, &analysis, &options, 1, 1, "audio"));
        }
        for (av, ov) in [(2, 1), (1, 2)] {
            assert_ne!(
                original,
                hash(&[1, 2, 1], &analysis, &options, av, ov, "audio")
            );
        }
        assert_ne!(
            original,
            hash(&[1, 2, 1], &analysis, &options, 1, 1, "replaced audio")
        );
        let mut changed = options.clone();
        changed.stem_playback = true;
        assert_ne!(
            original,
            hash(&[1, 2, 1], &analysis, &changed, 1, 1, "audio")
        );
        changed = options.clone();
        changed.live_moves = mixless_protocol::LiveMoves::Active;
        assert_ne!(
            original,
            hash(&[1, 2, 1], &analysis, &changed, 1, 1, "audio")
        );
        analysis.duration_sec = 121.;
        assert_ne!(
            original,
            hash(&[1, 2, 1], &analysis, &options, 1, 1, "audio")
        );
    }
}
