//! Shared vertical fader geometry: values refer to the cap's center stripe.

pub const CAP_HEIGHT: f32 = 26.0;

#[derive(Clone, Copy)]
pub struct FaderLane {
    top: f32,
    travel: f32,
}

impl FaderLane {
    pub fn new(top: f32, height: f32) -> Self {
        Self {
            top: top + CAP_HEIGHT / 2.0,
            travel: (height - CAP_HEIGHT).max(0.0),
        }
    }

    pub fn center(self, value: f32) -> f32 {
        self.top + (1.0 - value.clamp(0.0, 1.0)) * self.travel
    }

    pub fn value(self, y: f32) -> f32 {
        if self.travel == 0.0 {
            return 0.5;
        }
        (1.0 - (y - self.top) / self.travel).clamp(0.0, 1.0)
    }

    pub fn grab_offset(self, y: f32, value: f32) -> Option<f32> {
        let offset = y - self.center(value);
        (offset.abs() <= CAP_HEIGHT / 2.0).then_some(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_zero_is_at_bottom_and_tempo_neutral_is_centered() {
        use crate::state::FaderCtl;
        use mixless_protocol::DeckId;

        let lane = FaderLane::new(100.0, 226.0);
        for deck in [DeckId::A, DeckId::B] {
            let channel = FaderCtl::Channel(deck);
            assert_eq!(channel.range(), (0.0, 1.0, 0.01));
            assert_eq!(channel.reset_value(), 0.0);
            assert_eq!(lane.center(channel.reset_value()), 313.0);
            assert_eq!(lane.center(1.0), 113.0);

            let tempo = FaderCtl::Tempo(deck);
            let (min, max, _) = tempo.range();
            assert_eq!(tempo.reset_value(), 1.0);
            assert!((lane.center((tempo.reset_value() - min) / (max - min)) - 213.0).abs() < 1e-4);
        }
    }

    #[test]
    fn painted_stripe_maps_back_to_value_at_every_height() {
        for height in [120.0, 154.0, 220.0, 340.0] {
            let lane = FaderLane::new(137.0, height);
            for value in [0.0, 0.2, 0.5, 0.8, 1.0] {
                assert!((lane.value(lane.center(value)) - value).abs() < 1e-6);
            }
            assert_eq!(lane.value(0.0), 1.0);
            assert_eq!(lane.value(1000.0), 0.0);
        }
    }

    #[test]
    fn grabbing_cap_preserves_value_and_relative_movement() {
        let lane = FaderLane::new(100.0, 226.0);
        for value in [0.0, 0.5, 0.8, 1.0] {
            for offset in [-12.0, 0.0, 12.0] {
                let y = lane.center(value) + offset;
                let grab = lane.grab_offset(y, value).unwrap();
                assert!((lane.value(y - grab) - value).abs() < 1e-6);
                assert!((lane.value(y - grab - 20.0) - (value + 0.1).min(1.0)).abs() < 1e-6);
            }
        }
        assert!(lane.grab_offset(lane.center(0.8) + 14.0, 0.8).is_none());
    }

    #[test]
    fn collapsed_lane_stays_finite() {
        for height in [0.0, 20.0, CAP_HEIGHT] {
            let lane = FaderLane::new(10.0, height);
            assert!(lane.value(lane.center(0.8)).is_finite());
        }
    }
}
