//! Plan every adjacent pair of a playlist the way desktop AUTO does and
//! summarize how often the result is a boundary cut or a played-out track.
//! cargo run -p mixless-mixplan --features trace --example automix-audit -- ANALYSES.json [--stems] [--summary] [--best]
use mixless_mixplan::{PlanContext, Planner, PlannerOptions, short_handoff};
use mixless_protocol::{MixPlan, PerformanceOffset, TrackAnalysis};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .ok_or("Usage: automix-audit ANALYSES.json [--stems] [--summary] [--best] [--json]")?;
    let stems = args.iter().any(|a| a == "--stems");
    let json_out = args.iter().any(|a| a == "--json");
    let quiet = args.iter().any(|a| a == "--summary") || json_out;
    let mut tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(path)?)?;
    if args.iter().any(|a| a == "--best") {
        let refs: Vec<_> = tracks.iter().collect();
        let order = mixless_mixplan::order::best_cycle(
            &refs,
            &PlannerOptions {
                harmonic_key_shift: true,
                stem_playback: stems,
                ..Default::default()
            },
        );
        if let Ok(out) = std::env::var("AUDIT_ORDER_OUT") {
            std::fs::write(out, serde_json::to_vec(&order)?)?;
        }
        tracks = order.into_iter().map(|i| tracks[i].clone()).collect();
    }
    let (mut cuts, mut played_out, mut fallbacks, mut overlaps, mut bars) = (0, 0, 0, 0, 0u32);
    let mut offset = PerformanceOffset::identity();
    let mut entry = 0.;
    // Like the app: a track cannot start leaving before its own entrance has
    // finished, plus one second of staging.
    let mut earliest = 0f32;
    // Continue from an actually completed handoff, including a solo retune.
    if let Ok(path) = std::env::var("AUDIT_PREVIOUS_PLAN") {
        let previous: MixPlan = serde_json::from_slice(&std::fs::read(path)?)?;
        if previous.summary.as_ref().map(|s| s.pair.1) != tracks.first().map(|t| t.track_id) {
            return Err("Previous plan does not enter the first audit track".into());
        }
        offset = previous.incoming_offset_end;
        entry = previous.t_in_b;
        earliest = previous.t_end_b + offset.rate;
    }
    let mut sites = std::collections::BTreeMap::<&str, u32>::new();
    let mut pair_data = Vec::new();
    let mut plans = Vec::new();
    for (i, pair) in tracks.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        let ctx = PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: &[],
            cues_in: &[],
            offset_a: offset,
            offset_b: PerformanceOffset::identity(),
        };
        let planner = Planner::with_options(PlannerOptions {
            harmonic_key_shift: true,
            outgoing_entry_sec: entry,
            earliest_outgoing_sec: earliest,
            stem_playback: stems,
            ..Default::default()
        });
        let mut plan = planner.plan_with_following(&ctx, tracks.get(i + 2));
        let rejected = mixless_mixplan::trace::take();
        let fallback = plan.failure_reason.is_some();
        if fallback {
            plan = short_handoff(&ctx, earliest);
        }
        let summary = describe(&plan);
        // Reaching the last 8% (or 8 s) of A without a real overlap is playing
        // the track out; a long blend that ends there is mixing through the outro.
        let length = plan.summary.as_ref().map_or(0, |s| s.length_bars);
        let terminal =
            plan.t_out_a >= (a.duration_sec * 0.92).min(a.duration_sec - 8.) && length < 4;
        // The engine accepts a zero-bar plan only as a boundary cut.
        if plan.summary.as_ref().is_some_and(|s| {
            s.length_bars == 0
                && !matches!(
                    s.strategy,
                    mixless_protocol::StrategyId::DryCut | mixless_protocol::StrategyId::DropCut
                )
        }) {
            eprintln!(
                "pair {i}: zero-bar plan that is not a cut: {:?}",
                plan.summary
            );
        }
        plans.push(plan.clone());
        cuts += (length == 0) as u32;
        played_out += terminal as u32;
        fallbacks += fallback as u32;
        overlaps += (length >= 4) as u32;
        bars += length as u32;

        if let Some(s) = &plan.summary {
            pair_data.push(serde_json::json!({
                "pair": [a.track_id, b.track_id],
                "strategy": format!("{:?}", s.strategy).to_lowercase(),
                "score": s.score,
                "used_fallback": fallback,
                "length_bars": s.length_bars,
                "tempo_a": a.tempo.global_bpm,
                "tempo_b": b.tempo.global_bpm,
            }));
        }

        if quiet {
            offset = plan.incoming_offset_end;
            entry = plan.t_in_b;
            earliest = plan.t_end_b + plan.incoming_offset_end.rate;
            continue;
        }
        println!(
            "{i:2} {:6.1}->{:6.1}  {summary:28} {length:3} bars  out {:5.1}/{:5.1}s{}{}",
            a.tempo.global_bpm,
            b.tempo.global_bpm,
            plan.t_out_a,
            a.duration_sec,
            if fallback { "  FALLBACK" } else { "" },
            if terminal { "  PLAYED OUT" } else { "" },
        );
        let detail = std::env::var("PAIR")
            .ok()
            .and_then(|p| p.parse::<usize>().ok())
            == Some(i);
        for (site, count) in rejected.iter().take(if detail { 40 } else { 3 }) {
            println!("      {count:6} {site}");
            *sites.entry(site).or_default() += count;
        }
        offset = plan.incoming_offset_end;
        entry = plan.t_in_b;
        earliest = plan.t_end_b + plan.incoming_offset_end.rate;
    }

    if let Ok(out) = std::env::var("AUDIT_PLANS_OUT") {
        std::fs::write(out, serde_json::to_vec(&plans)?)?;
    }
    if json_out {
        let output = serde_json::json!({
            "pairs": pair_data,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let pairs = tracks.len().saturating_sub(1).max(1) as f32;
    println!(
        "\n{} pairs: cut {:.0}%  played out {}  fallback {}  >=4 bar overlap {:.0}%  mean {:.1} bars",
        pairs,
        100. * cuts as f32 / pairs,
        played_out,
        fallbacks,
        100. * overlaps as f32 / pairs,
        bars as f32 / pairs
    );
    if quiet {
        return Ok(());
    }
    let mut sites: Vec<_> = sites.into_iter().collect();
    sites.sort_by_key(|s| std::cmp::Reverse(s.1));
    for (site, count) in sites.iter().take(8) {
        println!("  {count:7} {site}");
    }
    Ok(())
}

fn describe(plan: &MixPlan) -> String {
    match &plan.summary {
        Some(s) => format!(
            "{:?}/{:?}{}",
            plan.transition_mode,
            s.strategy,
            plan.send_fx
                .map_or(String::new(), |fx| format!("+{:?}", fx.kind))
        )
        .replace("Some(", "")
        .replace(')', ""),
        None => plan.failure_reason.clone().unwrap_or_default(),
    }
}
