use super::*;
use crate::composition::Fade;

#[test]
fn prepared_reductions_match_original_cost_bits_for_success_and_rejection() {
    let mut seed = 19u32;
    let mut next = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.
    };
    let frames: Vec<_> = (0..129)
        .map(|i| Frame {
            u: i as f32 / 8.,
            sec: i as f32 / 4.,
            bands: std::array::from_fn(|_| {
                std::array::from_fn(|_| std::array::from_fn(|_| 0.0001 + next() * 0.01))
            }),
            voice: [0., 0.],
        })
        .collect();
    let (mut accepted, mut rejected, mut shared) = (0, 0, 0);
    for tonality in [Tonality::Compatible, Tonality::Unknown, Tonality::Conflict] {
        let prepared = Prepared::new(&frames, tonality);
        for i in 0..160 {
            let mut shape: Shape = std::array::from_fn(|d| {
                std::array::from_fn(|s| {
                    let start = ((i * (s + 1) + d * 3) % 60) as f32 / 4.;
                    Fade {
                        start,
                        end: (start + (s + 1) as f32 / 4.).min(16.),
                    }
                })
            });
            if i % 4 == 0 {
                shape = [[Fade { start: 4., end: 8. }; 4]; 2];
            }
            let expected = reference::cost(&frames, &shape, tonality).map(f32::to_bits);
            let actual = prepared.score(&shape).ok().map(f32::to_bits);
            assert_eq!(actual, expected, "shape {i}, {tonality:?}");
            if matches!(prepared.score(&shape), Err(Rejection::Shared)) {
                shared += 1;
                for offset in [-4., 0., 4., 20.] {
                    let mut changed = shape;
                    changed[0][1] = Fade {
                        start: offset,
                        end: offset + 0.5,
                    };
                    changed[1][1] = Fade {
                        start: offset - 2.,
                        end: offset + 4.,
                    };
                    assert_eq!(reference::cost(&frames, &changed, tonality), None);
                }
            }
            if actual.is_some() {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
    }
    assert!(accepted >= 40 && rejected >= 40);
    assert!(shared >= 40);
}
