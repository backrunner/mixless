//! Evaluate four independent lanes together, keeping each lane's arithmetic
//! order and exact division. Reductions and transcendental functions stay scalar.
use super::super::Shape;

#[cfg(not(all(target_arch = "aarch64", target_feature = "neon")))]
#[inline]
pub(super) fn gains(shape: &Shape, u: f32) -> [[f32; 4]; 2] {
    super::super::gains(shape, u)
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
#[inline]
pub(super) fn gains(shape: &Shape, u: f32) -> [[f32; 4]; 2] {
    use std::arch::aarch64::*;
    let mut result = [[0.; 4]; 2];
    // SAFETY: NEON is enabled for this target. Loads/stores each address four
    // initialized f32 values in local arrays; no layout cast or alias is used.
    unsafe {
        let zero = vdupq_n_f32(0.);
        let one = vdupq_n_f32(1.);
        let position = vdupq_n_f32(u);
        for d in 0..2 {
            let starts = shape[d].map(|f| f.start);
            let ends = shape[d].map(|f| f.end);
            let start = vld1q_f32(starts.as_ptr());
            let end = vld1q_f32(ends.as_ptr());
            let x = vdivq_f32(vsubq_f32(position, start), vsubq_f32(end, start));
            // Comparisons/selects preserve signed zero and NaN behavior of
            // f32::clamp; floating min/max instructions would change -0 to +0.
            let x = vbslq_f32(vcltq_f32(x, zero), zero, x);
            let x = vbslq_f32(vcgtq_f32(x, one), one, x);
            let inner = vaddq_f32(vdupq_n_f32(-15.), vmulq_f32(vdupq_n_f32(6.), x));
            let inner = vaddq_f32(vdupq_n_f32(10.), vmulq_f32(x, inner));
            let smooth = vmulq_f32(vmulq_f32(vmulq_f32(x, x), x), inner);
            let step = vbslq_f32(vcgeq_f32(position, end), one, zero);
            let value = vbslq_f32(vcleq_f32(end, start), step, smooth);
            let value = if d == 0 { vsubq_f32(one, value) } else { value };
            vst1q_f32(result[d].as_mut_ptr(), value);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::Fade;

    #[test]
    fn lanes_match_scalar_division_clamping_steps_and_rounding() {
        let mut seed = 23u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16777216.
        };
        for _ in 0..10000 {
            let shape = std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    let start = next() * 40. - 4.;
                    Fade {
                        start,
                        end: start + next() * 12. - 1.,
                    }
                })
            });
            let u = next() * 40.;
            assert_eq!(
                gains(&shape, u).map(|g| g.map(f32::to_bits)),
                super::super::super::gains(&shape, u).map(|g| g.map(f32::to_bits))
            );
        }
        for start in [-0., 0., f32::MIN_POSITIVE, -1., 1., 16., f32::MAX] {
            for end in [start, start.next_up(), -start, 0., 16., f32::MAX] {
                let shape = [[Fade { start, end }; 4]; 2];
                for u in [
                    start.next_down(),
                    start,
                    start.next_up(),
                    end,
                    end.next_up(),
                ] {
                    let actual = gains(&shape, u);
                    let expected = super::super::super::gains(&shape, u);
                    for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
                        assert!(a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()));
                    }
                }
            }
        }
    }
}
