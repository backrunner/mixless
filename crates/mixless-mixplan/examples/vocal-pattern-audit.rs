//! Export local repeat evidence from stored analyses without touching a library.
use mixless_protocol::{vocal_pattern, TrackAnalysis};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Expected ANALYSES.json OUTPUT.json".into());
    }
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let rows: Vec<_> = tracks.iter().map(|t| {
        let samples: Vec<_> = (0..(t.duration_sec * 2.) as usize).filter_map(|i| {
            let sec = (i as f32 + 0.5) * 0.5;
            vocal_pattern(t, sec).map(|p| serde_json::json!({"sec":sec, "period":p.period_sec,
                "envelope":p.envelope_similarity,"spectral":p.spectral_distance,"notes":p.note_similarity,
                "window":[p.start_sec,p.end_sec]}))
        }).collect();
        serde_json::json!({"track_id":t.track_id,"duration":t.duration_sec,"samples":samples})
    }).collect();
    std::fs::write(&args[1], serde_json::to_vec_pretty(&rows)?)?;
    Ok(())
}
