//! List the live moves the between-transition performer would schedule for
//! each track in a library database, from the persisted analyses.
//!
//! cargo run --release -p mixless-library --example live-moves-audit -- library-copy.db [ANALYSIS_VERSION]
//!
//! Run it on a COPY of the database; the example never writes, but the desktop
//! may hold the live file open in WAL mode.
use mixless_library::Library;
use mixless_mixplan::{performance_moves, LiveMoves, PerformanceLane};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(db) = args.first() else {
        return Err("Usage: live-moves-audit LIBRARY.db [ANALYSIS_VERSION]".into());
    };
    let version: u32 = args.get(1).map_or(Ok(16), |v| v.parse())?;
    let level = match std::env::var("MOVES_LEVEL").as_deref() {
        Ok("active") => LiveMoves::Active,
        Ok("off") => LiveMoves::Off,
        _ => LiveMoves::Subtle,
    };
    let library = Library::open(Path::new(db))?;
    for track in library.list_tracks()? {
        let Some(t) = library.load_analysis(track.id, version)? else {
            continue;
        };
        let moves = performance_moves(&t, 0., t.duration_sec, true, level);
        println!("{} {} ({:.0}s)", track.id.0, track.title, t.duration_sec);
        for m in moves {
            let lane = match m.lane {
                PerformanceLane::Filter => "filter".to_string(),
                PerformanceLane::Stem(s) => format!("stem/{s:?}"),
                PerformanceLane::Eq(band) => format!("eq/{band:?}"),
            };
            let peak = (0..=200)
                .map(|i| {
                    m.curve
                        .sample(m.start_sec + (m.end_sec - m.start_sec) * i as f32 / 200.)
                })
                .fold(0f32, f32::max);
            println!(
                "    {:6.1}-{:6.1}s {:14} {} (peak {peak:.2})",
                m.start_sec, m.end_sec, lane, m.label
            );
        }
    }
    Ok(())
}
