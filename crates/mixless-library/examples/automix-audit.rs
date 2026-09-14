//! Audit the transitions the smooth planner would choose for every adjacent
//! pair in each playlist of a library database, using the persisted analyses.
//!
//! cargo run --release -p mixless-library --example automix-audit -- library-copy.db [ANALYSIS_VERSION]
//!
//! Run it on a COPY of the database; the example never writes, but the desktop
//! may hold the live file open in WAL mode.
use mixless_library::Library;
use mixless_mixplan::{PlanContext, Planner, PlannerOptions};
use mixless_protocol::{Cue, CueKind, PlaylistId, SectionLabel, TrackAnalysis};
use std::{collections::BTreeMap, error::Error, path::Path};

fn section(t: &TrackAnalysis, sec: f32) -> SectionLabel {
    t.sections
        .iter()
        .find(|s| sec >= s.start_sec && sec < s.end_sec)
        .map_or(SectionLabel::Unknown, |s| s.label)
}

/// Vocal evidence around `sec`: separated-stem activity when present, else the
/// mixed-signal bar estimate. Returns (activity, source).
fn vocal(t: &TrackAnalysis, sec: f32) -> (f32, &'static str) {
    if let Some(frame) = t.stems.as_ref().and_then(|s| s.frame_at(sec)) {
        return (frame.vocal_activity, "stem");
    }
    let bar = t
        .bars
        .iter()
        .find(|b| sec >= b.start_sec && sec < b.end_sec)
        .map_or(0., |b| b.vocal_confidence.unwrap_or(b.vocal_presence));
    (bar, "bar")
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(db) = args.first() else {
        return Err("Usage: automix-audit LIBRARY.db [ANALYSIS_VERSION]".into());
    };
    let version: u32 = args.get(1).map_or(Ok(14), |v| v.parse())?;
    let library = Library::open(Path::new(db))?;
    let planner = Planner::with_options(PlannerOptions {
        harmonic_key_shift: true,
        stem_playback: std::env::var("MIXLESS_AUDIT_STEM_PLAYBACK").as_deref() == Ok("1"),
        ..Default::default()
    });
    let mut strategies: BTreeMap<String, usize> = BTreeMap::new();
    let (mut pairs, mut failed, mut short_with_vocal, mut stem_pairs) = (0, 0, 0, 0);
    // Per-track grid diagnostics: how much of each song the planner may treat
    // as a reliable clock, and why segments lose confidence.
    println!("== Tempo grids");
    for track in library.list_tracks()? {
        let Some(t) = library.load_analysis(track.id, version)? else {
            continue;
        };
        let tempo = &t.tempo;
        let segments = &tempo.segments;
        let confidence = |i: usize| {
            tempo
                .pulse_confidence
                .get(i)
                .copied()
                .unwrap_or(segments[i].confidence)
        };
        let reliable = (0..segments.len())
            .filter(|i| confidence(*i) >= 0.65)
            .count();
        let weak = (0..segments.len())
            .filter(|i| confidence(*i) < 0.35)
            .count();
        let bpms: Vec<String> = segments
            .iter()
            .enumerate()
            .map(|(i, s)| format!("{:.1}/{:.2}", s.bpm, confidence(i)))
            .collect();
        println!(
            "  {} | global {:.2} conf {:.2} | {} beats, {} downbeats, {} segments: {} reliable(>=0.65), {} weak(<0.35) | phrases {} | sections {}",
            track.title,
            tempo.global_bpm,
            segments.first().map_or(0., |s| s.confidence),
            tempo.beats.len(),
            tempo.downbeats.len(),
            segments.len(),
            reliable,
            weak,
            t.phrase_boundaries.len(),
            t.sections
                .iter()
                .map(|s| format!("{:?}@{:.0}", s.label, s.start_sec))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!("      segments bpm/conf: {}", bpms.join(" "));
    }
    for playlist in library.list_playlists()? {
        let tracks = library.playlist_tracks(PlaylistId(playlist.id))?;
        if tracks.len() < 2 {
            continue;
        }
        println!("\n== {} ({} tracks)", playlist.name, tracks.len());
        for pair in tracks.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (Some(ta), Some(tb)) = (
                library.load_analysis(a.id, version)?,
                library.load_analysis(b.id, version)?,
            ) else {
                println!("  {} -> {}: analysis missing", a.title, b.title);
                continue;
            };
            pairs += 1;
            let cues_out: Vec<Cue> = library
                .cues(a.id)?
                .into_iter()
                .filter(|c| c.kind != CueKind::In)
                .collect();
            let cues_in: Vec<Cue> = library
                .cues(b.id)?
                .into_iter()
                .filter(|c| c.kind != CueKind::Out)
                .collect();
            let plan = planner.plan_next(&PlanContext {
                outgoing: &ta,
                incoming: &tb,
                cues_out: &cues_out,
                cues_in: &cues_in,
                offset_a: Default::default(),
                offset_b: Default::default(),
            });
            let stems = ta.stems.is_some() && tb.stems.is_some();
            stem_pairs += usize::from(stems);
            let Some(summary) = &plan.summary else {
                failed += 1;
                println!(
                    "  {} -> {}: FAIL {}",
                    a.title,
                    b.title,
                    plan.failure_reason.as_deref().unwrap_or("?")
                );
                continue;
            };
            *strategies
                .entry(format!("{:?}", summary.strategy))
                .or_default() += 1;
            let exit = plan.t_out_a;
            let (va, va_src) = vocal(&ta, exit + 0.15);
            let (vb, vb_src) = vocal(&tb, plan.t_in_b + 0.25);
            let n = summary.length_bars;
            if n < 4 && va >= 0.5 {
                short_with_vocal += 1;
            }
            println!(
                "  {} -> {}: {:?}/{:?} n={} score={:.2}{}{} | A {:.1}-{:.1}s of {:.1} ({:?}) | B in {:.1}s ({:?}) | vocal A@exit+.15 {:.2}{} B@entry+.25 {:.2}{} | stems {}{}",
                a.title,
                b.title,
                plan.transition_mode,
                summary.strategy,
                n,
                summary.score,
                if summary.used_fallback { " FALLBACK" } else { "" },
                if plan.stem_mix.is_some() { " stem-env" } else { "" },
                plan.t_in_a,
                exit,
                ta.duration_sec,
                section(&ta, exit - 0.01),
                plan.t_in_b,
                section(&tb, plan.t_in_b + 0.01),
                va,
                va_src,
                vb,
                vb_src,
                stems,
                if n < 4 && va >= 0.5 { " <-- SHORT CUT ON VOCAL" } else { "" }
            );
        }
    }
    println!("\n== Summary: {pairs} pairs, {failed} failed, {stem_pairs} with stem evidence on both sides");
    for (strategy, count) in strategies {
        println!("  {strategy}: {count}");
    }
    println!("  short (<4 bars) transitions with the outgoing vocal still sounding past the exit: {short_with_vocal}");
    Ok(())
}
