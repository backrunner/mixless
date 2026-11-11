//! Unsmeared source evidence for heard passage and continuity accounting.
use mixless_protocol::{MixPlan, TrackAnalysis};

pub(super) struct Sample {
    pub raw: [f32; 4],
    pub heard: [f32; 4],
    pub voice: bool,
}

pub(super) fn reference(t: &TrackAnalysis) -> Option<f32> {
    let key = t as *const TrackAnalysis as usize;
    if let Some(value) =
        REFERENCES.with(|r| r.get().and_then(|r| r.into_iter().find(|v| v.0 == key)))
    {
        return value.1;
    }
    measure_reference(t)
}

fn measure_reference(t: &TrackAnalysis) -> Option<f32> {
    let mut powers: Vec<_> = t
        .stems
        .as_ref()?
        .frames
        .iter()
        .map(|f| {
            f.band_db
                .iter()
                .flatten()
                .map(|db| 10f32.powf(*db / 10.))
                .sum::<f32>()
        })
        .collect();
    let peak = powers.iter().copied().fold(0f32, f32::max);
    powers.retain(|v| v.is_finite() && *v > peak * 0.001);
    powers.sort_by(f32::total_cmp);
    powers.get(powers.len().saturating_sub(1) * 3 / 4).copied()
}

type References = Option<[(usize, Option<f32>); 2]>;
thread_local! { static REFERENCES: std::cell::Cell<References> = const { std::cell::Cell::new(None) }; }

/// Immutable pair-local evidence only; nested forecasts restore their caller.
pub(crate) struct Scope<'a>(
    References,
    std::marker::PhantomData<(&'a TrackAnalysis, std::rc::Rc<()>)>,
);
pub(crate) struct Evidence<'a>(References, std::marker::PhantomData<&'a TrackAnalysis>);
impl<'a> Evidence<'a> {
    pub(crate) fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self(
            Some([a, b].map(|t| (t as *const _ as usize, reference(t)))),
            std::marker::PhantomData,
        )
    }
    pub(crate) fn enter(&self) -> Scope<'a> {
        Scope(
            REFERENCES.with(|r| r.replace(self.0)),
            std::marker::PhantomData,
        )
    }
}
#[cfg(all(test, feature = "trace"))]
impl<'a> Scope<'a> {
    pub(crate) fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Evidence::new(a, b).enter()
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        REFERENCES.with(|r| r.set(self.0));
    }
}

pub(super) fn sample(p: &MixPlan, t: &TrackAnalysis, side: usize, u: f32) -> Option<Sample> {
    let source = p.source_seconds_at(side == 1, u);
    t.stems.as_ref()?;
    if source >= t.duration_sec {
        return Some(Sample {
            raw: [0.; 4],
            heard: [0.; 4],
            voice: false,
        });
    }
    let (frame, power) = crate::level::evidence::frame(t, source)?;
    let x = ((p.lanes.xfader.sample(u) + 1.) * 0.5).clamp(0., 1.);
    let (fader, gain, eq, filter, stems) = if side == 0 {
        (
            1. - x,
            &p.lanes.gain_a,
            &p.lanes.eq_a,
            &p.lanes.filter_a,
            p.stem_mix.as_ref().map(|s| &s.outgoing),
        )
    } else {
        (
            if u + 0.0001 >= p.incoming_start_bar {
                x
            } else {
                0.
            },
            &p.lanes.gain_b,
            &p.lanes.eq_b,
            &p.lanes.filter_b,
            p.stem_mix.as_ref().map(|s| &s.incoming),
        )
    };
    let mut result = Sample {
        raw: [0.; 4],
        heard: [0.; 4],
        voice: frame.vocal_activity >= 0.2,
    };
    let eq = [eq.low.sample(u), eq.mid.sample(u), eq.high.sample(u)];
    let gain = gain.sample(u);
    let lp = filter.lp_hz.sample_log(u).max(1.);
    let hp = filter.hp_hz.sample_log(u).max(1.);
    let filter_power =
        [70f32, 600., 5000.].map(|hz| 1. / ((1. + (hz / lp).powi(4)) * (1. + (hp / hz).powi(4))));
    let gain_power = eq.map(|eq| 10f32.powf((gain + eq) / 10.));
    let stem_gain: [f32; 4] =
        std::array::from_fn(|part| stems.map_or(1., |s| s[part].sample(u).clamp(0., 1.)));
    for stem in 0..3 {
        for band in 0..3 {
            let part = if stem == 2 && band == 0 { 3 } else { stem };
            let raw = power[stem][band];
            result.raw[part] += raw;
            result.heard[part] +=
                raw * fader * gain_power[band] * stem_gain[part].powi(2) * filter_power[band];
        }
    }
    Some(result)
}

pub(super) fn bar_at(p: &MixPlan, sec: f32) -> f32 {
    let source = &p.incoming_source;
    let i = source
        .nodes
        .partition_point(|v| v.1 < sec)
        .min(source.nodes.len().saturating_sub(1));
    if i == 0 {
        return source.nodes.first().map_or(0., |v| v.0);
    }
    let (a, b) = (source.nodes[i - 1], source.nodes[i]);
    a.0 + (b.0 - a.0) * ((sec - a.1) / (b.1 - a.1).max(1e-6)).clamp(0., 1.)
}
