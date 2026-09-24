//! Rank candidates only after musical and source constraints have passed.
use super::*;

pub(super) struct Evidence {
    pub start: f32,
    pub end: f32,
    pub bin: f32,
    pub n: f32,
    pub mode: TransitionMode,
    pub explicit_out: bool,
    pub phrases: [f32; 4],
    pub voices: [f32; 2],
    pub rms: [Option<f32>; 2],
    pub key: f32,
    pub key_shift: f32,
    pub clash: f32,
    pub drop_cut: bool,
    pub scratch_cut: bool,
    pub spinback: bool,
    pub loop_out: bool,
    /// Stems are aligned on both decks, so a looped-out strip-down is a
    /// strictly safer handoff than sharing the full outgoing spectrum.
    pub stem_pair: bool,
    pub fx_decision: policy::Decision,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(levels: [f32; 2], master_scale: f32) -> f32 {
        let a = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            64,
            0.9,
            0.2 * master_scale,
        );
        let b = crate::tests::track(
            2,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Intro,
            64,
            0.9,
            0.2 * master_scale,
        );
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        Evidence {
            start: 96.,
            end: 128.,
            bin: 0.,
            n: 16.,
            mode: TransitionMode::BeatBlend,
            explicit_out: false,
            phrases: [1.; 4],
            voices: [0.1; 2],
            rms: levels.map(|r| Some(r * master_scale)),
            key: 1.,
            key_shift: 0.,
            clash: 0.,
            drop_cut: false,
            scratch_cut: false,
            spinback: false,
            loop_out: false,
            stem_pair: false,
            fx_decision: policy::Decision {
                technique: Technique::DryCut,
                confidence: 1.,
            },
        }
        .score(&ctx, &PlannerOptions::default())
        .unwrap()
    }

    #[test]
    fn equally_quiet_windows_rank_below_a_pair_with_one_strong_source() {
        let quiet = score([0.05, 0.05], 1.);
        assert!(score([0.05, 0.2], 1.) > quiet + 0.1);
        assert!(score([0.2, 0.2], 1.) > quiet + 0.1);
        assert!((score([0.05, 0.05], 0.25) - quiet).abs() < 1e-6);
        assert!(score([0.0001, 0.0001], 1.) <= quiet);
    }
}
impl Evidence {
    pub fn score(self, ctx: &PlanContext<'_>, options: &PlannerOptions) -> Option<f32> {
        let Self {
            start,
            end,
            bin,
            n,
            mode,
            explicit_out,
            phrases: [out_phrase, in_phrase, start_phrase, end_phrase],
            voices: [vocal_a, vocal_b],
            rms: [rms_a, rms_b],
            key,
            key_shift,
            clash,
            drop_cut,
            scratch_cut,
            spinback,
            loop_out,
            stem_pair,
            fx_decision,
        } = self;
        let (a, b) = (ctx.outgoing, ctx.incoming);
        let (sa, sb) = (section(a, end - 0.001), section(b, bin + 0.001));
        let blend = mode == TransitionMode::BeatBlend;
        let loop_roll = mode == TransitionMode::LoopRoll;
        let structural = matches!(
            section(a, end - 0.001),
            mixless_protocol::SectionLabel::Outro
                | mixless_protocol::SectionLabel::Break
                | mixless_protocol::SectionLabel::Drop
                | mixless_protocol::SectionLabel::Chorus
        );
        let recovery_overlap = !drop_cut
            && (crate::recovery::contains(a, start, end)
                || (matches!(
                    section(a, start + 0.01),
                    mixless_protocol::SectionLabel::Break
                        | mixless_protocol::SectionLabel::Breakdown
                        | mixless_protocol::SectionLabel::BuildUp
                ) && a
                    .sections
                    .iter()
                    .filter(|s| s.start_sec < end && s.end_sec > start)
                    .all(|s| crate::drops::recovery(s.label))
                    && matches!(
                        sb,
                        mixless_protocol::SectionLabel::Intro
                            | mixless_protocol::SectionLabel::BuildUp
                            | mixless_protocol::SectionLabel::Break
                            | mixless_protocol::SectionLabel::Breakdown
                    )));
        let position = (if recovery_overlap { 0.12 } else { 0. })
            - 0.03 * bin / b.duration_sec
            - if explicit_out {
                0.
            } else {
                crate::drops::penalty(a, end, options.outgoing_entry_sec)
            };
        // Phrase-compatible pairings are preferred even when BPM/key are equal.
        // The vocal rule is hard: a verse and a chorus with two foregrounds never
        // share the same overlap window.
        if n < 4.
            && sa == mixless_protocol::SectionLabel::Verse
            && sb == mixless_protocol::SectionLabel::Chorus
            && vocal_a > 0.45
            && vocal_b > 0.45
        {
            return None;
        }
        let structural_score = match (sa, sb) {
            (mixless_protocol::SectionLabel::Outro, mixless_protocol::SectionLabel::Intro)
            | (mixless_protocol::SectionLabel::Drop, mixless_protocol::SectionLabel::Intro)
            | (mixless_protocol::SectionLabel::BuildUp, mixless_protocol::SectionLabel::Drop)
            | (mixless_protocol::SectionLabel::Break, mixless_protocol::SectionLabel::Intro) => 1.0,
            (mixless_protocol::SectionLabel::Chorus, mixless_protocol::SectionLabel::Intro)
            | (mixless_protocol::SectionLabel::Breakdown, mixless_protocol::SectionLabel::Intro) => {
                0.85
            }
            (mixless_protocol::SectionLabel::Unknown, _)
            | (_, mixless_protocol::SectionLabel::Unknown) => 0.55,
            _ => 0.35,
        };
        let quality = if blend {
            // A looped-out handoff is a genuine improvement on the same blend
            // window, so it scores a touch above a plain swap.
            0.57 + 0.10 * key + 0.10 * structural_score - 0.10 * clash
                + if loop_out { 0.04 } else { 0. }
                + if loop_out && stem_pair { 0.04 } else { 0. }
        } else if loop_roll {
            0.39 + 0.08 * structural_score + 0.05 * (1. - vocal_a.min(vocal_b))
        } else {
            0.35 + 0.10 * structural_score
                + 0.02 * fx_decision.confidence
                + 0.08 * (1. - vocal_a.min(vocal_b))
                + if drop_cut { 0.08 } else { 0. }
                + if spinback { 0.06 } else { 0. }
        };
        let phrase = if blend {
            (out_phrase + in_phrase + start_phrase + end_phrase) / 4.
        } else {
            (out_phrase + in_phrase) / 2.
        };
        let energy = crate::energy::balance([rms_a, rms_b]);
        let quiet_pair = if drop_cut {
            0.
        } else {
            crate::energy::weakness(a, rms_a).min(crate::energy::weakness(b, rms_b))
        };
        let kick = match (feature(a, end - 0.01), feature(b, bin + 0.01)) {
            (Some(a), Some(b)) => 1. - (a.kick_salience - b.kick_salience).abs(),
            _ => 0.5,
        };
        // A short bridge is a fallback. A complete, compatible recovery overlap
        // earns useful duration even while preserving a vocal foreground.
        let length_fit = if blend {
            // Judge sustained compatibility across the whole interval. A useful
            // long blend earns room to develop, with no penalty beyond 24/32 bars.
            let compatibility = (0.55 + 0.45 * key) * (1. - clash * 0.5);
            0.24 * (1. - (-n / 20.).exp()) * compatibility
        } else if recovery_overlap && !drop_cut && !scratch_cut && n >= 4. {
            0.10 * (1. - (-n / 16.).exp())
        } else {
            0.
        };
        let score =
            ((quality + position + phrase * 0.12 + energy * 0.05 + kick * 0.03 + length_fit
                - key_shift.abs() * 0.025
                - quiet_pair * 0.25
                + if structural { 0.03 } else { 0. })
                / 1.4)
                .clamp(0., 1.);
        Some(score)
    }
}
