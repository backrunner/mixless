//! Export the same per-bar/section intensity descriptors used by Best order.
//! energy-audit ANALYSES.json OUTPUT.json
use mixless_protocol::{bar_energy, energy_reference, musical_energy, track_energy, TrackAnalysis};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Expected ANALYSES.json OUTPUT.json".into());
    }
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let values: Vec<_> = tracks.iter().map(|t| serde_json::json!({
        "track_id": t.track_id, "active_drive_db": track_energy(t),
        "sections": t.sections.iter().map(|s| serde_json::json!({ "start_sec": s.start_sec, "end_sec": s.end_sec, "label": s.label, "energy": musical_energy(t, s.start_sec, s.end_sec) })).collect::<Vec<_>>(),
        "bars": t.bars.iter().map(|b| serde_json::json!({ "start_sec": b.start_sec, "end_sec": b.end_sec, "energy": bar_energy(b, energy_reference(t)) })).collect::<Vec<_>>()
    })).collect();
    std::fs::write(&args[1], serde_json::to_vec_pretty(&values)?)?;
    Ok(())
}
