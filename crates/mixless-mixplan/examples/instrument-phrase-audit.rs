//! Export note-derived exchange evidence; never writes the library.
use mixless_protocol::{instrument_boundaries, instrument_voices, TrackAnalysis};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("Expected ANALYSES.json OUTPUT.json".into());
    }
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let rows: Vec<_> = tracks
        .iter()
        .map(|t| {
            serde_json::json!({
                "track_id":t.track_id,"boundaries":instrument_boundaries(t),"voices":instrument_voices(t)
            })
        })
        .collect();
    std::fs::write(&args[1], serde_json::to_vec_pretty(&rows)?)?;
    Ok(())
}
