//! Host writes are copied with try_lock once per block, never waited on by audio.
use super::*;
use mixless_protocol::{MasterChainMeters, MasterChainSettings, MasterStageMeter};

pub(super) struct MasterShared {
    pub settings: Mutex<MasterChainSettings>,
    meters: [AtomicU32; 17],
}

impl MasterShared {
    pub fn new() -> Self {
        let settings = Mutex::new(MasterChainSettings::default());
        // macOS pthread Mutex lazily allocates on its first lock, including
        // try_lock. Initialize it on the host before the callback can see it.
        drop(settings.lock().expect("master settings"));
        Self {
            settings,
            meters: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
    pub fn publish(&self, meters: MasterChainMeters, decay: f32) {
        let mut values = [0.; 17];
        for (i, stage) in [meters.compressor, meters.dynamics, meters.limiter]
            .into_iter()
            .enumerate()
        {
            values[i * 5..i * 5 + 5].copy_from_slice(&[
                stage.input[0],
                stage.input[1],
                stage.output[0],
                stage.output[1],
                stage.reduction_db,
            ]);
        }
        values[15..].copy_from_slice(&meters.rms);
        for (i, (slot, value)) in self.meters.iter().zip(values).enumerate() {
            let previous = f32::from_bits(slot.load(Ordering::Relaxed));
            slot.store(
                if i < 15 {
                    value.max(previous * decay)
                } else {
                    value
                }
                .to_bits(),
                Ordering::Relaxed,
            );
        }
    }
    pub fn read(&self) -> MasterChainMeters {
        let v: [f32; 17] =
            std::array::from_fn(|i| f32::from_bits(self.meters[i].load(Ordering::Relaxed)));
        let stage = |i: usize| MasterStageMeter {
            input: [v[i], v[i + 1]],
            output: [v[i + 2], v[i + 3]],
            reduction_db: v[i + 4],
        };
        MasterChainMeters {
            compressor: stage(0),
            dynamics: stage(5),
            limiter: stage(10),
            rms: [v[15], v[16]],
        }
    }
}
