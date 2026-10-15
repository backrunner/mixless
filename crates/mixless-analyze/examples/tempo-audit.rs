//! Summarize beat-grid confidence per track, the evidence Automix uses to
//! decide whether two decks may share a beat.
//! cargo run --release -p mixless-analyze --example tempo-audit -- [--json OUT] [--full OUT] [--vocals on] TRACK...
use mixless_analyze::{AnalysisOptions, Analyzer};
use mixless_protocol::TrackId;
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let mut take = |flag: &str| {
        let i = args.iter().position(|a| a == flag)?;
        args.remove(i);
        Some(args.remove(i))
    };
    let json = take("--json");
    let full = take("--full");
    let vocals = take("--vocals").is_some_and(|v| v == "on");
    if args.is_empty() {
        return Err("Usage: tempo-audit [--json OUT] [--full OUT] [--vocals on] TRACK...".into());
    }
    let analyzer = Analyzer::with_options(AnalysisOptions {
        native_vocals: vocals,
        ..Default::default()
    });
    let mut maps = Vec::new();
    let mut analyses = Vec::new();
    for (i, path) in args.iter().enumerate() {
        let a = analyzer.analyze_track(TrackId(i as i64), Path::new(path))?;
        let t = &a.tempo;
        let pulse = &t.pulse_confidence;
        let reliable = pulse.iter().filter(|c| **c >= 0.65).count();
        let weak = pulse.iter().filter(|c| **c < 0.35).count();
        let mut bpms: Vec<_> = t
            .segments
            .iter()
            .map(|s| (s.bpm * 10.).round() / 10.)
            .collect();
        bpms.dedup();
        let segment = t.segments.iter().map(|s| s.confidence).fold(1f32, f32::min);
        println!(
            "{:2} {:6.2} BPM  pulse>=.65 {:2}/{:2}  <.35 {:2}  min segment {:.2}  tempi {:?}  {}",
            i,
            t.global_bpm,
            reliable,
            pulse.len(),
            weak,
            segment,
            bpms,
            Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        maps.push(a.tempo.clone());
        analyses.push(a);
    }
    if let Some(out) = full {
        std::fs::write(out, serde_json::to_vec(&analyses)?)?;
    }
    if let Some(out) = json {
        std::fs::write(out, serde_json::to_vec(&maps)?)?;
    }
    Ok(())
}
