//! Mixer control values, resets and drag gestures.
use super::*;

impl UiState {
    pub fn reset_knob(&mut self, ctl: KnobCtl) {
        let value = match ctl {
            KnobCtl::Stem(_, _) => 1.,
            KnobCtl::Master => 0.8,
            KnobCtl::MasterGain => 0.0,
            KnobCtl::Key(_) => 0.0,
            KnobCtl::Trim(_) | KnobCtl::Gain(_) => 0.0,
            KnobCtl::Balance(_) => 0.0,
            KnobCtl::Filter(_) => 0.0,
            KnobCtl::Resonance(_) => 0.35,
            KnobCtl::Eq(_, _) => 0.0,
            KnobCtl::FxMix(_, _) => 0.5,
            KnobCtl::FxParam(deck, slot, param) => {
                param.value(&FxState::new(self.fx[deck.index()][slot].kind))
            }
        };
        self.set_knob_value(ctl, value);
    }

    pub fn reset_fader(&mut self, ctl: FaderCtl) {
        self.end_drag();
        self.set_fader_value(ctl, ctl.reset_value());
    }

    pub fn reset_xfader(&mut self) {
        self.dispatch(Command::SetCrossfader { value: 0.0 });
    }

    pub(crate) fn apply_fx(&mut self, deck: DeckId, slot: usize) {
        let fx = &self.fx[deck.index()][slot];
        let params = fx.params();
        self.dispatch(Command::SetFx {
            deck: Some(deck),
            slot: fx_slot(slot),
            params,
        });
    }

    /* ---- drag dispatch --------------------------------------------------- */

    pub fn begin_knob(&mut self, ctl: KnobCtl, y: f32) {
        let (min, max, _) = ctl.range();
        let val = self.knob_value(ctl).clamp(min, max);
        self.drag = Some(DragCtl::Knob {
            ctl,
            start_y: y,
            start_val: val,
        });
    }

    pub fn begin_fader(&mut self, ctl: FaderCtl, y: f32, bounds: Bounds<Pixels>, displayed: f32) {
        let lane = FaderLane::new(bounds.origin.y.into(), bounds.size.height.into());
        let grab = lane.grab_offset(y, displayed);
        if grab.is_none() {
            let (min, max, _) = ctl.range();
            self.set_fader_value(ctl, min + lane.value(y) * (max - min));
        }
        self.drag = Some(DragCtl::Fader {
            ctl,
            lane,
            grab_offset: grab.unwrap_or(0.0),
        });
    }

    pub fn begin_xfader(&mut self, x: f32, bounds: Bounds<Pixels>) {
        let t = ((x - f32::from(bounds.origin.x)) / f32::from(bounds.size.width)).clamp(0.0, 1.0);
        let v = (t * 2.0 - 1.0).clamp(-1.0, 1.0);
        self.dispatch(Command::SetCrossfader { value: v });
        self.drag = Some(DragCtl::Xfader { bounds });
    }

    /// Begin a jog scrub. The platter bounds give the drag center; the
    /// revolution rate (33⅓ rpm → 1.8 s per revolution) makes one full
    /// circular gesture scrub exactly 1.8 s of audio, matching the marker.
    pub fn begin_jog(&mut self, deck: DeckId, x: f32, y: f32, bounds: Bounds<Pixels>) {
        self.end_drag();
        if self.deck(deck).frames == 0 {
            return;
        }
        self.focus = deck;
        self.dispatch(Command::SetJogTouch {
            deck,
            touching: true,
        });
        let (cx, cy) = (
            f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0,
            f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0,
        );
        let d = self.deck(deck);
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            self.snapshot.sample_rate
        };
        let r = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        let platter_radius =
            (f32::from(bounds.size.width).min(f32::from(bounds.size.height)) / 2.0 - 6.0).max(8.0);
        // Grabs on the label behave like a conventional vertical scratch;
        // grabs on the vinyl follow the pointer angle one-to-one.
        let dead_radius = (platter_radius * 0.30).max(18.0);
        let angular = r > dead_radius;
        let last_angle = angular.then(|| (y - cy).atan2(x - cx));
        self.drag = Some(DragCtl::Jog {
            deck,
            cx,
            cy,
            angular,
            last_angle,
            last_y: y,
            dead_radius,
            frames_per_rev: sr as f64 * 1.8,
        });
    }

    /// Begin a waveform scrub along its time axis.
    pub fn begin_wave(&mut self, deck: DeckId, vertical: bool, pos: f32, frames_per_px: f32) {
        self.end_drag();
        if self.deck(deck).frames == 0 {
            return;
        }
        self.focus = deck;
        self.dispatch(Command::SetJogTouch {
            deck,
            touching: true,
        });
        self.drag = Some(DragCtl::Wave {
            deck,
            vertical,
            frames_per_px,
            last_pos: pos,
        });
    }

    pub fn drag_move(&mut self, x: f32, y: f32) {
        match self.drag.take() {
            Some(DragCtl::Knob {
                ctl,
                start_y,
                start_val,
            }) => {
                let (min, max, step) = ctl.range();
                let dv = ((start_y - y) / 120.0) * (max - min);
                let v = (start_val + dv).clamp(min, max);
                let v = (v / step).round() * step;
                self.set_knob_value(ctl, v.clamp(min, max));
                self.drag = Some(DragCtl::Knob {
                    ctl,
                    start_y,
                    start_val,
                });
            }
            Some(DragCtl::Fader {
                ctl,
                lane,
                grab_offset,
            }) => {
                let (min, max, _) = ctl.range();
                let t = lane.value(y - grab_offset);
                self.set_fader_value(ctl, min + t * (max - min));
                self.drag = Some(DragCtl::Fader {
                    ctl,
                    lane,
                    grab_offset,
                });
            }
            Some(DragCtl::Xfader { bounds }) => {
                let t = ((x - f32::from(bounds.origin.x)) / f32::from(bounds.size.width))
                    .clamp(0.0, 1.0);
                let v = (t * 2.0 - 1.0).clamp(-1.0, 1.0);
                self.dispatch(Command::SetCrossfader { value: v });
                self.drag = Some(DragCtl::Xfader { bounds });
            }
            Some(DragCtl::Jog {
                deck,
                cx,
                cy,
                angular,
                mut last_angle,
                mut last_y,
                dead_radius,
                frames_per_rev,
            }) => {
                // Keep the mode chosen at mouse-down. While an angular drag
                // crosses the center dead zone, re-anchor without seeking so
                // it cannot produce a discontinuity on the far side.
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt();
                let frames = if angular && r > dead_radius {
                    let cur = dy.atan2(dx);
                    let mut d = match last_angle {
                        Some(prev) => cur - prev,
                        None => 0.0,
                    };
                    // Wrap to (-π, π]; clamp guards the discontinuity when
                    // the pointer sweeps across the center.
                    while d > std::f32::consts::PI {
                        d -= 2.0 * std::f32::consts::PI;
                    }
                    while d < -std::f32::consts::PI {
                        d += 2.0 * std::f32::consts::PI;
                    }
                    last_angle = Some(cur);
                    (d as f64 / (2.0 * std::f64::consts::PI)) * frames_per_rev
                } else if angular {
                    last_angle = None;
                    0.0
                } else {
                    (last_y - y) as f64 * 180.0 * frames_per_rev / (44_100.0 * 1.8)
                };
                if frames.abs() >= 0.01 {
                    self.dispatch(Command::Jog {
                        deck,
                        delta_frames: frames as f32,
                    });
                }
                last_y = y;
                self.drag = Some(DragCtl::Jog {
                    deck,
                    cx,
                    cy,
                    angular,
                    last_angle,
                    last_y,
                    dead_radius,
                    frames_per_rev,
                });
            }
            Some(DragCtl::Wave {
                deck,
                vertical,
                frames_per_px,
                mut last_pos,
            }) => {
                // Grab the waveform: moving content right/down moves source
                // time backward under the fixed playhead in either layout.
                let delta = last_pos - if vertical { y } else { x };
                let frames = delta * frames_per_px;
                if frames.abs() >= 0.01 {
                    self.dispatch(Command::Jog {
                        deck,
                        delta_frames: frames,
                    });
                }
                last_pos = if vertical { y } else { x };
                self.drag = Some(DragCtl::Wave {
                    deck,
                    vertical,
                    frames_per_px,
                    last_pos,
                });
            }
            None => {}
        }
    }

    pub fn end_drag(&mut self) {
        self.pending_drag = None;
        if let Some(DragCtl::Jog { deck, .. } | DragCtl::Wave { deck, .. }) = self.drag.take() {
            self.dispatch(Command::SetJogTouch {
                deck,
                touching: false,
            });
        }
    }

    pub fn knob_value(&self, ctl: KnobCtl) -> f32 {
        match ctl {
            KnobCtl::Stem(d, stem) => self.deck(d).stem_gain[stem.index()],
            KnobCtl::Master => self.snapshot.master,
            KnobCtl::MasterGain => self.snapshot.master_gain_db,
            KnobCtl::Key(d) => self.deck(d).pitch_semitones,
            KnobCtl::Trim(d) => self.deck(d).gain_db,
            KnobCtl::Gain(d) => self.deck(d).effective_limiter_gain_db(),
            KnobCtl::Balance(d) => self.deck(d).balance,
            KnobCtl::Filter(d) => self.deck(d).filter_amount,
            KnobCtl::Resonance(d) => self.deck(d).filter_resonance,
            KnobCtl::Eq(d, band) => self.deck(d).eq_db[band.index()],
            KnobCtl::FxMix(d, s) => self.fx[d.index()][s].mix,
            KnobCtl::FxParam(d, s, param) => param.value(&self.fx[d.index()][s]),
        }
    }

    pub(super) fn set_knob_value(&mut self, ctl: KnobCtl, v: f32) {
        match ctl {
            KnobCtl::Stem(deck, stem) => self.dispatch(Command::SetStemGain {
                deck,
                stem,
                value: v,
            }),
            KnobCtl::Master => self.dispatch(Command::SetMaster { value: v }),
            KnobCtl::MasterGain => self.dispatch(Command::SetMasterGain { db: v }),
            KnobCtl::Key(d) => self.dispatch(Command::SetPitchSemitones {
                deck: d,
                semitones: v,
            }),
            KnobCtl::Trim(d) => self.dispatch(Command::SetChannelGain { deck: d, db: v }),
            KnobCtl::Gain(d) => self.dispatch(Command::SetDeckLimiterGain { deck: d, db: v }),
            KnobCtl::Balance(d) => self.dispatch(Command::SetBalance { deck: d, value: v }),
            KnobCtl::Filter(d) => self.dispatch(Command::SetChannelFilter { deck: d, amount: v }),
            KnobCtl::Resonance(d) => self.dispatch(Command::SetFilterResonance {
                deck: d,
                resonance: v,
            }),
            KnobCtl::Eq(d, band) => self.dispatch(Command::SetEq {
                deck: d,
                band,
                db: v,
            }),
            KnobCtl::FxMix(d, s) => self.set_fx_mix(d, s, v),
            KnobCtl::FxParam(d, s, param) => {
                param.set(&mut self.fx[d.index()][s], v);
                self.apply_fx(d, s);
            }
        }
    }

    pub(super) fn set_fader_value(&mut self, ctl: FaderCtl, v: f32) {
        match ctl {
            FaderCtl::Tempo(d) => self.dispatch(Command::SetRate { deck: d, rate: v }),
            FaderCtl::Channel(d) => self.dispatch(Command::SetChannelFader { deck: d, value: v }),
        }
    }

    /* ---- text inputs ------------------------------------------------------ */
}
