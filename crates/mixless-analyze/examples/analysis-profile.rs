//! cargo run --release -p mixless-analyze --example analysis-profile -- [--dsp-only] TRACK...
//! Profile with /usr/bin/time -l target/release/examples/analysis-profile TRACK.
use mixless_analyze::{AnalysisOptions, Analyzer};
use mixless_protocol::TrackId;
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let dsp_only = args.first().is_some_and(|a| a == "--dsp-only");
    if dsp_only {
        args.remove(0);
    }
    if args.is_empty() {
        return Err("Usage: analysis-profile [--dsp-only] TRACK...".into());
    }
    let analyzer = Analyzer::with_options(AnalysisOptions {
        native_vocals: !dsp_only,
        ..Default::default()
    });
    for (i, path) in args.iter().enumerate() {
        let (a, r) = analyzer.analyze_track_with_report(TrackId(i as i64), Path::new(path))?;
        println!("file={} duration={:.3}s decode={:.1}ms features={:.1}ms vocals={:.1}ms status={:?} sections={} phrases={} voice_bars={}/{}",
            i+1, a.duration_sec, r.decode_time.as_secs_f64()*1000., r.feature_time.as_secs_f64()*1000.,
            r.vocal_time.as_secs_f64()*1000., r.vocal_status, a.sections.len(),a.phrase_boundaries.len(),
            a.bars.iter().filter(|b| b.vocal_confidence.is_some()).count(),a.bars.len());
        for s in &a.sections {
            println!("  {:.3}-{:.3}s {:?}", s.start_sec, s.end_sec, s.label);
        }
        println!(
            "  model_voice_peak={:.3} voiced_bars={}",
            a.bars
                .iter()
                .filter_map(|b| b.vocal_confidence)
                .fold(0., f32::max),
            a.bars
                .iter()
                .filter(|b| b.vocal_confidence.is_some_and(|c| c >= 0.5))
                .count()
        );
    }
    Ok(())
}
