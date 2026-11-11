use crate::{PlanContext, PlannerOptions};
use mixless_protocol::{Cue, PerformanceOffset, StrategyId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Key {
    tracks: [usize; 2],
    cues: [Vec<(u8, u64, u8, bool)>; 2],
    offsets: [u32; 4],
    collect: bool,
    options: Options,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Options {
    harmonic: bool,
    smooth: bool,
    who: u8,
    literal: bool,
    strategy: Option<StrategyId>,
    earliest: u32,
    entry: u32,
    stems: bool,
    moves: u8,
}

impl Key {
    pub(super) fn new(ctx: &PlanContext<'_>, options: &PlannerOptions, collect: bool) -> Self {
        // New planner inputs must explicitly join the key when fields are added.
        let PlanContext {
            outgoing,
            incoming,
            cues_out,
            cues_in,
            offset_a,
            offset_b,
        } = ctx;
        let PerformanceOffset {
            rate: rate_a,
            pitch_semitones: pitch_a,
        } = offset_a;
        let PerformanceOffset {
            rate: rate_b,
            pitch_semitones: pitch_b,
        } = offset_b;
        let PlannerOptions {
            harmonic_key_shift,
            smooth,
            who_stretches,
            literal_half_double,
            strategy,
            earliest_outgoing_sec,
            outgoing_entry_sec,
            stem_playback,
            live_moves,
        } = options;
        let cues = |cues: &[Cue]| {
            cues.iter()
                .map(|c| {
                    let Cue {
                        index,
                        frame,
                        kind,
                        user_set,
                    } = c;
                    (*index, *frame, *kind as u8, *user_set)
                })
                .collect()
        };
        Self {
            tracks: [
                *outgoing as *const _ as usize,
                *incoming as *const _ as usize,
            ],
            cues: [cues(cues_out), cues(cues_in)],
            offsets: [
                rate_a.to_bits(),
                pitch_a.to_bits(),
                rate_b.to_bits(),
                pitch_b.to_bits(),
            ],
            collect,
            options: Options {
                harmonic: *harmonic_key_shift,
                smooth: *smooth,
                who: *who_stretches as u8,
                literal: *literal_half_double,
                strategy: *strategy,
                earliest: earliest_outgoing_sec.to_bits(),
                entry: outgoing_entry_sec.to_bits(),
                stems: *stem_playback,
                moves: *live_moves as u8,
            },
        }
    }

    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .cues
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<(u8, u64, u8, bool)>())
                .sum::<usize>()
    }
}
