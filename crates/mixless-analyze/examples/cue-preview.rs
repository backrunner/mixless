//! Usage: cue-preview ANALYSES.json — print the automatic cues of each track.
use mixless_protocol::TrackAnalysis;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("analyses path")?;
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(path)?)?;
    for t in &tracks {
        let cues: Vec<String> = mixless_analyze::automatic_cues(t)
            .iter()
            .map(|c| format!("{:?}@{:.1}", c.kind, c.frame as f32 / t.sample_rate as f32))
            .collect();
        println!("{} {}", t.track_id.0, cues.join(" "));
    }
    Ok(())
}
