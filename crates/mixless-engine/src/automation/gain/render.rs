//! Independent host preview using the production automation and audio graph.
use super::*;
use meter::{Meter, Reading};

pub(super) struct Preview {
    pub readings: Vec<Reading>,
    pub history: Vec<Reading>,
    pub output: Output,
}

#[derive(Clone)]
pub(super) struct Output {
    pub levels: Vec<meter::Metric>,
    pub bands: Vec<[f64; 3]>,
    pub step_lag: usize,
}

pub(super) fn render(
    dense: &DensePlan,
    slots: [&DeckSlot; 2],
    shared: &Shared,
    reference: bool,
    guard: &Guard<'_>,
) -> Option<Preview> {
    let sr = dense.sample_rate;
    let engine = Engine::new(EngineConfig {
        sample_rate: sr,
        block_frames: STEP as u32,
        offline: true,
    })
    .ok()?;
    let dest = &engine.shared;
    dest.fx_auto_fade.store(
        shared.fx_auto_fade.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
    for (d, source) in slots.into_iter().enumerate() {
        let slot = &dest.decks[d];
        let audio = source.buffer.lock().ok()?.clone()?;
        *slot.buffer.lock().ok()? = Some(audio.clone());
        *slot.stems.lock().ok()? = source.stems.lock().ok()?.clone();
        *slot.beat_grid.lock().ok()? = source.beat_grid.lock().ok()?.clone();
        slot.frames.store(audio.frames, Ordering::Relaxed);
        slot.src_sr.store(audio.sample_rate, Ordering::Relaxed);
        slot.track_id
            .store(source.track_id.load(Ordering::Relaxed), Ordering::Relaxed);
        for (to, from) in [
            (&slot.rate_micro, &source.rate_micro),
            (&slot.pitch_centi, &source.pitch_centi),
            (&slot.fader, &source.fader),
            (&slot.gain_milli, &source.gain_milli),
            (&slot.limiter_gain_centi, &source.limiter_gain_centi),
            (&slot.balance_milli, &source.balance_milli),
            (&slot.filter_milli, &source.filter_milli),
            (&slot.resonance_milli, &source.resonance_milli),
            (&slot.send_milli, &source.send_milli),
            (&slot.bpm_milli, &source.bpm_milli),
        ] {
            to.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for (to, from) in [
            (&slot.keylock, &source.keylock),
            (&slot.resonance_enabled, &source.resonance_enabled),
        ] {
            to.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for i in 0..3 {
            slot.eq_db[i].store(source.eq_db[i].load(Ordering::Relaxed), Ordering::Relaxed);
            slot.eq_kill[i].store(source.eq_kill[i].load(Ordering::Relaxed), Ordering::Relaxed);
        }
        for i in 0..4 {
            slot.stem_gain[i].store(
                source.stem_gain[i].load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            let saved = source.inserts[i].save();
            slot.inserts[i].revision.store(
                source.inserts[i].revision.load(Ordering::Acquire),
                Ordering::Release,
            );
            slot.inserts[i].restore(&saved);
        }
    }
    for (to, from) in [
        (&dest.master, &shared.master),
        (&dest.master_gain_centi, &shared.master_gain_centi),
    ] {
        to.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    *dest.master_chain.settings.lock().ok()? = *shared.master_chain.settings.lock().ok()?;
    for i in 0..2 {
        dest.sends[i].revision.store(
            shared.sends[i].revision.load(Ordering::Acquire),
            Ordering::Release,
        );
        dest.sends[i].restore(&shared.sends[i].save());
    }
    let mut preview = dense.clone();
    preview.outgoing = 0;
    preview.gain_revision = None;
    if reference {
        // A counterfactual for the gain budget only. Source clocks, routing,
        // planned FX and every strategy decision remain identical.
        for point in &mut preview.points {
            for deck in &mut point.decks {
                deck.stems = deck.stems.map(|_| [1000; 4]);
                deck.eq = [9600; 3];
                deck.filter = 500;
                deck.compensation = 0;
                deck.tone_compensation = [0; 7];
            }
        }
    }
    dest.xfader.store(0, Ordering::Relaxed);
    let rate = slots[0].rate_micro.load(Ordering::Relaxed) as f64 / 1_000_000.;
    let source_sr = slots[0].src_sr.load(Ordering::Relaxed) as f64;
    // Whole blocks give identical launch phase in all three previews. Keep
    // three seconds of history for short-term loudness and stateful DSP.
    let preroll =
        ((dense.start_frame / (source_sr * rate)).min(3.) * sr as f64) as usize / STEP * STEP;
    dest.decks[0]
        .set_playhead((dense.start_frame - preroll as f64 / sr as f64 * source_sr * rate).max(0.));
    dest.decks[0].playing.store(true, Ordering::Relaxed);
    engine.commit_plan(PreparedMix(Arc::new(preview))).ok()?;
    let hop = ((HOP * sr as f32) as usize / STEP).max(1) * STEP;
    let mut meter = Meter::new(sr, hop)?;
    let mut readings = Vec::new();
    let mut history = Vec::new();
    let mut measured_output = Output {
        levels: Vec::new(),
        bands: Vec::new(),
        step_lag: hop / STEP,
    };
    let mut rt = engine.rt.lock().ok()?;
    let mut output = [0.; STEP * 2];
    let mut observed = [[0.; 8]; STEP];
    let frames = preroll + dense.overlap_frames + (3 * sr as usize);
    let mut pending = 0;
    let mut phase = 1;
    let mut peaks = [meter::Metric::default(); 5];
    for frame in (0..frames).step_by(STEP) {
        if frame % hop == 0 && !guard.valid() {
            return None;
        }
        dest.process_block_observed::<true>(&mut rt, &mut output, 2, &mut observed);
        if output
            .iter()
            .chain(observed.iter().flatten())
            .any(|x| !x.is_finite())
        {
            return None;
        }
        let m = &rt.master_chain.meters;
        meter.push(
            &observed,
            &output,
            m.compressor.reduction_db.max(m.limiter.reduction_db),
        );
        pending += STEP;
        // Fit at 200 ms, but validate 200 ms changes at four window offsets.
        // Quantize each boundary independently so rates such as 44.1 kHz keep
        // the exact original coarse hop and do not accumulate clock drift.
        let boundary = frame + STEP == preroll;
        let threshold = (hop / STEP * phase).div_ceil(4) * STEP;
        if pending >= threshold || boundary {
            let mut reading = meter.finish()?;
            if frame + STEP > preroll {
                measured_output.levels.push(reading[4]);
                measured_output.bands.append(&mut meter.output_bands);
            } else {
                meter.output_bands.clear();
            }
            for d in 0..5 {
                peaks[d].peak = peaks[d].peak.max(reading[d].peak);
                peaks[d].reduction = peaks[d].reduction.max(reading[d].reduction);
            }
            if pending >= hop || boundary {
                for d in 0..5 {
                    reading[d].peak = peaks[d].peak;
                    reading[d].reduction = peaks[d].reduction;
                }
                if frame + STEP > preroll {
                    readings.push(reading);
                } else {
                    history.push(reading);
                }
                peaks = [meter::Metric::default(); 5];
                pending = 0;
                phase = 1;
            } else {
                phase += 1;
            }
        }
    }
    Some(Preview {
        readings,
        history,
        output: measured_output,
    })
}
