//! Device-free audit of exported TrackAnalysis JSON; never modifies the library.
//! cargo run -p mixless-analyze --example structure-audit -- INPUT.json OUTPUT.json
use mixless_analyze::Analyzer;
use mixless_mixplan::{Planner, PlannerOptions};
use mixless_protocol::TrackAnalysis;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("Expected INPUT.json OUTPUT.json [AUDIO_PATHS.json]".into());
    }
    let mut tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    if let Some(paths) = args.get(2) {
        let paths: Vec<String> = serde_json::from_slice(&std::fs::read(paths)?)?;
        if paths.len() != tracks.len() {
            return Err("Audio paths must match input tracks".into());
        }
        for (track, path) in tracks.iter_mut().zip(paths) {
            *track = Analyzer::new().analyze_track(track.track_id, std::path::Path::new(&path))?;
        }
    }
    for track in &mut tracks {
        Analyzer::refresh_structure(track);
        println!(
            "track={} bpm={:.2}",
            track.track_id.0, track.tempo.global_bpm
        );
        for s in &track.sections {
            println!("  {:.2}-{:.2} {:?}", s.start_sec, s.end_sec, s.label);
        }
    }
    let planner = Planner::with_options(PlannerOptions {
        harmonic_key_shift: true,
        ..Default::default()
    });
    for i in 0..tracks.len() {
        let (a, b) = (&tracks[i], &tracks[(i + 1) % tracks.len()]);
        let mut p = planner.plan_pair(a, b, &[], &[], Default::default(), Default::default());
        if p.failure_reason.is_some() {
            p = mixless_mixplan::short_handoff(
                &mixless_mixplan::PlanContext {
                    outgoing: a,
                    incoming: b,
                    cues_out: &[],
                    cues_in: &[],
                    offset_a: Default::default(),
                    offset_b: Default::default(),
                },
                0.,
            );
        }
        println!(
            "pair={}→{} A={:.2}–{:.2} B={:.2}–{:.2} {:?} {:?}",
            a.track_id.0,
            b.track_id.0,
            p.t_in_a,
            p.t_out_a,
            p.t_in_b,
            p.t_end_b,
            p.summary,
            p.failure_reason
        );
    }
    std::fs::write(&args[1], serde_json::to_vec_pretty(&tracks)?)?;
    Ok(())
}
