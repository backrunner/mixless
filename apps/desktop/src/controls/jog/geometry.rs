//! Shared platter paint and hit geometry. Progress is clockwise from noon.
use gpui::{Bounds, Pixels};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlatterGeometry {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
}

impl PlatterGeometry {
    pub fn new(bounds: Bounds<Pixels>) -> Self {
        Self {
            cx: f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.,
            cy: f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.,
            radius: (f32::from(bounds.size.width).min(f32::from(bounds.size.height)) / 2. - 6.)
                .max(8.),
        }
    }

    pub fn distance(self, x: f32, y: f32) -> f32 {
        (x - self.cx).hypot(y - self.cy)
    }

    pub fn on_progress(self, x: f32, y: f32) -> bool {
        (self.distance(x, y) - self.radius * 0.95).abs() <= 6.
    }

    pub fn phase(self, x: f32, y: f32) -> Option<f64> {
        // Crossing the hub must not create an accidental half-track jump.
        (self.distance(x, y) >= self.radius * 0.30).then(|| {
            (((y - self.cy) as f64).atan2((x - self.cx) as f64) / std::f64::consts::TAU + 0.25)
                .rem_euclid(1.)
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgressDrag {
    pub geometry: PlatterGeometry,
    pub position: f64,
    last_phase: Option<f64>,
}

impl ProgressDrag {
    pub fn new(geometry: PlatterGeometry, x: f32, y: f32) -> Option<Self> {
        let position = geometry.phase(x, y)?;
        Some(Self {
            geometry,
            position,
            last_phase: Some(position),
        })
    }

    pub fn update(&mut self, x: f32, y: f32) -> f64 {
        let phase = self.geometry.phase(x, y);
        if let (Some(last), Some(next)) = (self.last_phase, phase) {
            let delta = (next - last + 0.5).rem_euclid(1.) - 0.5;
            self.position = (self.position + delta).clamp(0., 1.);
        }
        self.last_phase = phase;
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn geometry() -> PlatterGeometry {
        PlatterGeometry {
            cx: 100.,
            cy: 100.,
            radius: 90.,
        }
    }
    #[test]
    fn progress_is_clockwise_and_hit_area_excludes_hub_and_corners() {
        let g = geometry();
        for ((x, y), expected) in [
            ((100., 10.), 0.),
            ((190., 100.), 0.25),
            ((100., 190.), 0.5),
            ((10., 100.), 0.75),
        ] {
            assert!((g.phase(x, y).unwrap() - expected).abs() < 0.000001);
            assert!(g.on_progress(x, y));
        }
        assert!(!g.on_progress(100., 100.));
        assert!(!g.on_progress(0., 0.));
    }
    #[test]
    fn dragging_across_noon_clamps_at_endpoints_and_hub_reanchors() {
        let g = geometry();
        let mut end = ProgressDrag::new(g, 99., 10.).unwrap();
        assert_eq!(end.update(101., 10.), 1.);
        assert!(end.update(99., 10.) > 0.99);
        let mut start = ProgressDrag::new(g, 101., 10.).unwrap();
        assert_eq!(start.update(99., 10.), 0.);
        let mut drag = ProgressDrag::new(g, 190., 100.).unwrap();
        assert_eq!(drag.update(100., 100.), 0.25);
        assert_eq!(drag.update(10., 100.), 0.25);
        assert!(drag.update(10., 99.) > 0.25);
    }
}
