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
    pub fx_decision: policy::Decision,
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
            0.57 + 0.10 * key + 0.10 * structural_score - 0.10 * clash
        } else if loop_roll {
            0.39 + 0.08 * structural_score + 0.05 * (1. - vocal_a.min(vocal_b))
        } else {
            0.35 + 0.10 * structural_score
                + 0.02 * fx_decision.confidence
                + 0.08 * (1. - vocal_a.min(vocal_b))
                + if drop_cut { 0.08 } else { 0. }
        };
        let phrase = if blend {
            (out_phrase + in_phrase + start_phrase + end_phrase) / 4.
        } else {
            (out_phrase + in_phrase) / 2.
        };
        let energy = match (rms_a, rms_b) {
            (Some(a), Some(b)) => (1. - (20. * (a / b).log10()).abs() / 18.).clamp(0., 1.),
            _ => 0.5,
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
                + if structural { 0.03 } else { 0. })
                / 1.4)
                .clamp(0., 1.);
        Some(score)
    }
}
