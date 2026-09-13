//! Finite sample-clocked wet/dry envelope. Reversing a fade starts at the
//! current audible mix; repeated block configuration never restarts it.
pub(super) struct WetFade {
    sample_rate: f32,
    current: f32,
    from: f32,
    target: f32,
    elapsed: u32,
    duration: u32,
}

impl WetFade {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            current: 0.,
            from: 0.,
            target: 0.,
            elapsed: 1,
            duration: 1,
        }
    }

    pub fn set(&mut self, target: f32, seconds: f32) {
        if target.is_finite() && self.target != target {
            self.from = self.current;
            self.target = target;
            self.elapsed = 0;
            self.duration = (self.sample_rate * seconds).round().max(1.) as u32;
        }
    }

    pub fn next(&mut self) -> f32 {
        if self.elapsed < self.duration {
            self.elapsed += 1;
            let t = self.elapsed as f32 / self.duration as f32;
            self.current = self.from + (self.target - self.from) * t * t * (3. - 2. * t);
            if self.elapsed == self.duration {
                self.current = self.target;
            }
        }
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_is_bounded_reversible_and_independent_of_block_updates() {
        let mut fade = WetFade::new(48_000.);
        fade.set(0.8, 0.15);
        let mut previous = 0.;
        for _ in 0..3600 {
            fade.set(0.8, 0.15);
            let value = fade.next();
            assert!((0.0..=0.0002).contains(&(value - previous)));
            previous = value;
        }
        assert!((previous - 0.4).abs() < 0.00001);
        fade.set(0., 0.15);
        for _ in 0..7200 {
            let value = fade.next();
            assert!(value >= 0. && value <= previous);
            assert!(previous - value < 0.0002);
            previous = value;
        }
        assert_eq!(fade.next(), 0.);
        fade.set(1., 0.005);
        for _ in 0..240 {
            fade.next();
        }
        assert_eq!(fade.next(), 1.);
    }
}
