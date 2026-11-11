//! Factor audible part envelopes through the mixer's equal-power crossfader.
//! Channel level is cleared after routing has reached the receiving deck.

pub(crate) struct Routing {
    pub crossfader: f32,
    pub gain_db: f32,
    pub stems: [[f32; 4]; 2],
}

pub(crate) fn route(parts: [[f32; 4]; 2]) -> Routing {
    // Below -40 dB, finish the routing move instead of holding the fader in
    // the centre for a numerical envelope tail.
    let levels = parts.map(|p| p.into_iter().fold(0f32, f32::max));
    let levels = levels.map(|v| if v <= 0.01 { 0. } else { v });
    let power = levels[0].powi(2) + levels[1].powi(2);
    let gain_power = if (power - 1.).abs() < 1e-6 { 1. } else { power };
    let crossfader = if power > 0. {
        (levels[1].powi(2) - levels[0].powi(2)) / power
    } else {
        1.
    };
    Routing {
        crossfader,
        gain_db: if power > 0. {
            10. * gain_power.log10()
        } else {
            0.
        },
        stems: std::array::from_fn(|d| {
            parts[d].map(|v| if levels[d] > 0. { v / levels[d] } else { 0. })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_preserves_selected_parts_and_moves_before_channel_cleanup() {
        let mut previous = -1.;
        for step in 0..=100 {
            let a = 1. - step as f32 / 100.;
            let b = 1.;
            let parts = [[a, a * 0.5, 0., 0.], [0., b, b * 0.4, b]];
            let r = route(parts);
            assert!(r.crossfader >= previous);
            previous = r.crossfader;
            let cross = [
                ((1. - r.crossfader) / 2.).sqrt(),
                ((1. + r.crossfader) / 2.).sqrt(),
            ];
            for d in 0..2 {
                for s in 0..4 {
                    let heard = cross[d] * 10f32.powf(r.gain_db / 20.) * r.stems[d][s];
                    assert!((heard - parts[d][s]).abs() <= 0.01001);
                }
            }
            // The outgoing channel never has to close while the crossfader
            // still sends it to the master.
            assert!(r.gain_db >= -0.001);
        }
        assert_eq!(previous, 1.);
    }
}
