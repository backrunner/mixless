//! Master mixer editing and bounded live-meter history.
mod graphs;
mod panels;
mod params;
mod view;
use crate::state::{DragCtl, UiState};
use gpui::{Bounds, Pixels};
use mixless_protocol::{Command, MasterChainMeters, MasterChainSettings};
pub use params::MasterParam;
use std::{collections::VecDeque, time::Instant};

#[derive(Default)]
pub struct MasterMixer {
    pub open: bool,
    pub band: usize,
    pub history: VecDeque<MasterChainMeters>,
    last_sample: Option<Instant>,
    dirty: bool,
}

impl MasterMixer {
    pub fn sample(&mut self, meters: MasterChainMeters) {
        if !self.open
            || self
                .last_sample
                .is_some_and(|t| t.elapsed().as_millis() < 33)
        {
            return;
        }
        self.last_sample = Some(Instant::now());
        if self.history.len() >= 150 {
            self.history.pop_front();
        }
        self.history.push_back(meters);
    }
}

impl UiState {
    pub fn open_master_mixer(&mut self) {
        self.end_drag();
        self.fx_editor = None;
        self.master_mixer.open = true;
        self.master_mixer.history.clear();
    }
    pub fn close_master_mixer(&mut self) {
        self.end_drag();
        self.master_mixer.open = false;
    }
    pub fn set_master_chain(&mut self, p: MasterChainSettings) {
        match self
            .core
            .engine
            .dispatch(Command::SetMasterChain { settings: p })
        {
            Ok(()) => {
                self.snapshot.master_chain = p;
                self.master_mixer.dirty = true;
            }
            Err(e) => self.error = e.to_string().into(),
        }
    }
    pub fn save_master_chain(&mut self) {
        if !self.master_mixer.dirty {
            return;
        }
        let p = self.snapshot.master_chain;
        match self.core.settings.update(|s| s.master_chain = p) {
            Ok(()) => self.master_mixer.dirty = false,
            Err(e) => self.error = e.into(),
        }
    }
    pub fn set_master_param(&mut self, param: MasterParam, value: f32) {
        let mut p = self.snapshot.master_chain;
        param.set(&mut p, value);
        self.set_master_chain(p);
    }
    pub fn drag_master_eq(&mut self, band: usize, bounds: Bounds<Pixels>, x: f32, y: f32) {
        let mut p = self.snapshot.master_chain;
        let t = ((x - f32::from(bounds.origin.x)) / f32::from(bounds.size.width)).clamp(0., 1.);
        p.eq[band].frequency_hz = 20. * 1000_f32.powf(t);
        p.eq[band].gain_db = (12.
            - (y - f32::from(bounds.origin.y)) / f32::from(bounds.size.height) * 24.)
            .clamp(-12., 12.);
        self.set_master_chain(p);
    }
    pub fn begin_master_eq(&mut self, bounds: Bounds<Pixels>, x: f32, y: f32) {
        self.end_drag();
        let nearest = self
            .snapshot
            .master_chain
            .eq
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let distance = |v: &mixless_protocol::MasterEqBand| {
                    let bx = f32::from(bounds.origin.x)
                        + (v.frequency_hz / 20.).log10() / 3. * f32::from(bounds.size.width);
                    let by = f32::from(bounds.origin.y)
                        + (12. - v.gain_db) / 24. * f32::from(bounds.size.height);
                    (x - bx).powi(2) + (y - by).powi(2)
                };
                distance(a).total_cmp(&distance(b))
            })
            .map_or(0, |(i, _)| i);
        self.master_mixer.band = nearest;
        self.drag_master_eq(nearest, bounds, x, y);
        self.drag = Some(DragCtl::MasterEq {
            band: nearest,
            bounds,
        });
    }
}
