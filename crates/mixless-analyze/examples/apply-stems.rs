//! Usage: apply-stems RAW.json STEMMED.json OUT.json
//! Apply evidence once to raw analyses. Never reweight already adjusted bars.
use mixless_protocol::TrackAnalysis;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: apply-stems RAW.json STEMMED.json OUT.json".into());
    }
    let mut tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let evidence: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    for t in &mut tracks {
        if t.stems.is_some() {
            return Err(format!(
                "track {:?}: input must contain unmodified raw bars",
                t.track_id
            )
            .into());
        }
        let source = evidence
            .iter()
            .find(|s| s.track_id == t.track_id)
            .ok_or("missing stem evidence for a track")?;
        if source.sample_rate != t.sample_rate
            || (source.duration_sec - t.duration_sec).abs() > 0.001
        {
            return Err("stem evidence has different source timing".into());
        }
        let stems = source.stems.clone().ok_or("track has no stem evidence")?;
        if !mixless_analyze::Analyzer::apply_stems(t, stems) {
            return Err(format!("track {:?}: stems did not cover the track", t.track_id).into());
        }
    }
    std::fs::write(&args[2], serde_json::to_vec(&tracks)?)?;
    Ok(())
}
