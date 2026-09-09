//! Atomic FX parameter transfer between the host and audio callback.

use super::*;

impl AtomicFx {
    pub(super) fn new() -> Self {
        Self {
            kind: AtomicU32::new(0),
            mix: AtomicU32::new(0.5f32.to_bits()),
            bypass: AtomicBool::new(true),
            feedback: AtomicU32::new(0.35f32.to_bits()),
            beats: AtomicU32::new(0.5f32.to_bits()),
            rate: AtomicU32::new(0.0f32.to_bits()),
            depth: AtomicU32::new(0.5f32.to_bits()),
            drive: AtomicU32::new(0.5f32.to_bits()),
            decay: AtomicU32::new(1.6f32.to_bits()),
            size: AtomicU32::new(0.5f32.to_bits()),
            damping: AtomicU32::new(0.5f32.to_bits()),
        }
    }

    pub(super) fn set(&self, params: &FxParams, kind: EffectKind) {
        self.mix
            .store(params.mix.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.feedback.store(
            params.feedback.unwrap_or(0.35).clamp(0.0, 0.88).to_bits(),
            Ordering::Relaxed,
        );
        self.beats.store(
            params
                .time_beats
                .unwrap_or(kind.default_beats())
                .clamp(0.0625, kind.max_beats())
                .to_bits(),
            Ordering::Relaxed,
        );
        self.rate.store(
            params.rate_hz.unwrap_or(0.0).clamp(0.0, 30.0).to_bits(),
            Ordering::Relaxed,
        );
        self.depth.store(
            params.depth.unwrap_or(0.5).clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        self.drive.store(
            params.drive.unwrap_or(0.5).clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        self.decay.store(
            params
                .decay_seconds
                .unwrap_or(1.6)
                .clamp(0.2, 8.0)
                .to_bits(),
            Ordering::Relaxed,
        );
        self.size.store(
            params.size.unwrap_or(0.5).clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        self.damping.store(
            params.damping.unwrap_or(0.5).clamp(0.0, 1.0).to_bits(),
            Ordering::Relaxed,
        );
        self.kind.store(kind as u32, Ordering::Release);
    }

    pub(super) fn read(&self, bpm: f32) -> EffectParams {
        EffectParams {
            kind: EffectKind::from_id(self.kind.load(Ordering::Acquire)),
            mix: f32::from_bits(self.mix.load(Ordering::Relaxed)),
            bypass: self.bypass.load(Ordering::Relaxed),
            feedback: f32::from_bits(self.feedback.load(Ordering::Relaxed)),
            beats: f32::from_bits(self.beats.load(Ordering::Relaxed)),
            rate_hz: f32::from_bits(self.rate.load(Ordering::Relaxed)),
            bpm,
            depth: f32::from_bits(self.depth.load(Ordering::Relaxed)),
            drive: f32::from_bits(self.drive.load(Ordering::Relaxed)),
            decay_seconds: f32::from_bits(self.decay.load(Ordering::Relaxed)),
            size: f32::from_bits(self.size.load(Ordering::Relaxed)),
            damping: f32::from_bits(self.damping.load(Ordering::Relaxed)),
        }
    }

    pub(super) fn snapshot(&self) -> mixless_protocol::FxState {
        let p = self.read(120.0);
        mixless_protocol::FxState {
            kind: p.kind,
            on: !p.bypass,
            mix: p.mix,
            beats: p.beats,
            rate_hz: p.rate_hz,
            feedback: p.feedback,
            depth: p.depth,
            drive: p.drive,
            decay_seconds: p.decay_seconds,
            size: p.size,
            damping: p.damping,
        }
    }
}

pub(super) fn insert_index(slot: FxSlot) -> Option<usize> {
    match slot {
        FxSlot::Insert0 => Some(0),
        FxSlot::Insert1 => Some(1),
        FxSlot::Insert2 => Some(2),
        FxSlot::Insert3 => Some(3),
        FxSlot::SendEcho | FxSlot::SendReverb => None,
    }
}
