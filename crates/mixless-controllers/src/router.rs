//! Stateful per-device decoding: layers, pickup, button edges and HID counters.
use crate::*;
use mixless_protocol::EngineSnapshot;
use std::collections::HashMap;
#[derive(Default)]
struct BindingState {
    raw: Option<f64>,
    relative_remainder: f64,
    down: bool,
    latched: bool,
    picked: bool,
    last_software: Option<f64>,
}
#[derive(Default)]
pub(crate) struct Router {
    states: HashMap<(String, String), BindingState>,
    layers: HashMap<(String, String), bool>,
    held: HashMap<(String, String), MidiTarget>,
    decoders: HashMap<String, mixless_midi::Decoder>,
    scopes: HashMap<String, String>,
}

pub fn read_hid_field(
    data: &[u8],
    offset: u16,
    bits: u8,
    little: bool,
    signed: bool,
) -> Option<i64> {
    if !(1..=32).contains(&bits) || offset as usize + bits as usize > data.len() * 8 {
        return None;
    }
    let mut value = 0u64;
    for n in 0..bits as usize {
        let pos = offset as usize + n;
        let bit = if little {
            (data[pos / 8] >> (pos % 8)) & 1
        } else {
            (data[pos / 8] >> (7 - pos % 8)) & 1
        };
        let shift = if little { n } else { bits as usize - 1 - n };
        value |= (bit as u64) << shift;
    }
    if signed && value & (1 << (bits - 1)) != 0 {
        Some(value as i64 - (1i64 << bits))
    } else {
        Some(value as i64)
    }
}
pub fn write_report_field(
    data: &mut [u8],
    offset: u16,
    bits: u8,
    little: bool,
    value: u32,
) -> bool {
    if !(1..=32).contains(&bits) || offset as usize + bits as usize > data.len() * 8 {
        return false;
    }
    for n in 0..bits as usize {
        let pos = offset as usize + n;
        let shift = if little { n } else { bits as usize - 1 - n };
        let dest = if little { pos % 8 } else { 7 - pos % 8 };
        data[pos / 8] = (data[pos / 8] & !(1 << dest)) | ((((value >> shift) & 1) as u8) << dest);
    }
    true
}
impl Router {
    pub(super) fn is_held(&self, target: &MidiTarget) -> bool {
        self.held
            .values()
            .any(|held| held.holds_same_control(target))
    }

    pub fn release_device(&mut self, id: &str) -> Vec<MidiAction> {
        let keys: Vec<_> = self.held.keys().filter(|(d, _)| d == id).cloned().collect();
        let actions: Vec<_> = keys
            .into_iter()
            .filter_map(|k| self.held.remove(&k))
            .map(|target| MidiAction {
                target,
                value: MidiValue::Release,
            })
            .collect();
        self.states.retain(|(d, _), _| d != id);
        self.layers.retain(|(d, _), _| d != id);
        self.decoders.remove(id);
        self.scopes.remove(id);
        actions
            .into_iter()
            .filter(|a| !self.is_held(&a.target))
            .collect()
    }
    pub(super) fn release_preset(&mut self, preset: &str) -> Vec<(String, MidiAction)> {
        let prefix = format!("{preset}:");
        let keys: Vec<_> = self
            .held
            .keys()
            .filter(|(_, binding)| binding.starts_with(&prefix))
            .cloned()
            .collect();
        let mut actions = Vec::new();
        for key in keys {
            if let Some(target) = self.held.remove(&key) {
                actions.push((
                    key.0,
                    MidiAction {
                        target,
                        value: MidiValue::Release,
                    },
                ));
            }
        }
        self.states
            .retain(|(_, binding), _| !binding.starts_with(&prefix));
        actions.retain(|(_, a)| !self.is_held(&a.target));
        actions
    }
    pub fn release_all(&mut self) -> Vec<(String, MidiAction)> {
        let actions = self
            .held
            .drain()
            .map(|((id, _), target)| {
                (
                    id,
                    MidiAction {
                        target,
                        value: MidiValue::Release,
                    },
                )
            })
            .collect();
        self.states.clear();
        self.layers.clear();
        self.decoders.clear();
        self.scopes.clear();
        actions
    }
    pub fn route(
        &mut self,
        device: &str,
        endpoint: &str,
        preset: &ControllerPreset,
        hid: bool,
        data: &[u8],
        snapshot: &EngineSnapshot,
    ) -> Vec<MidiAction> {
        self.route_for_controller(device, device, endpoint, preset, hid, data, snapshot)
    }
    pub fn route_for_controller(
        &mut self,
        scope: &str,
        device: &str,
        endpoint: &str,
        preset: &ControllerPreset,
        hid: bool,
        data: &[u8],
        snapshot: &EngineSnapshot,
    ) -> Vec<MidiAction> {
        self.scopes.insert(device.into(), scope.into());
        if !hid {
            self.decoders
                .entry(device.into())
                .or_default()
                .receive(data);
        }
        let mut result = Vec::new();
        // Modifiers are evaluated before controls in the same HID report.
        let bindings = preset.bindings.iter().filter(|b| b.endpoint == endpoint);
        let ordered: Vec<_> = bindings
            .clone()
            .filter(|b| b.sets_layer.is_some())
            .chain(bindings.filter(|b| b.sets_layer.is_none()))
            .collect();
        for b in ordered {
            let value = match &b.source {
                InputSource::Midi { address } if !hid => self
                    .decoders
                    .get_mut(device)
                    .and_then(|d| d.value(address))
                    .map(|(v, max)| (v as f64, 0.0, max as f64)),
                InputSource::Hid {
                    report_id,
                    bit_offset,
                    bits,
                    signed,
                    little_endian,
                } if hid && data.first() == Some(report_id) => {
                    read_hid_field(data, *bit_offset, *bits, *little_endian, *signed).map(|v| {
                        (
                            v as f64,
                            if *signed {
                                -(1i64 << (*bits - 1)) as f64
                            } else {
                                0.
                            },
                            if *signed {
                                ((1i64 << (*bits - 1)) - 1) as f64
                            } else {
                                ((1u64 << *bits) - 1) as f64
                            },
                        )
                    })
                }
                _ => None,
            };
            let Some((raw, min, max)) = value else {
                continue;
            };
            if let Some(layer) = &b.sets_layer {
                self.layers.insert((scope.into(), layer.clone()), raw != 0.);
                continue;
            }
            let key = (device.to_string(), format!("{}:{}", preset.id, b.id));
            let active = b.layer.as_ref().is_none_or(|l| {
                self.layers
                    .get(&(scope.into(), l.name.clone()))
                    .copied()
                    .unwrap_or(false)
                    == l.active
            });
            if !active {
                if let Some(target) = self.held.remove(&key) {
                    result.push(MidiAction {
                        target,
                        value: MidiValue::Release,
                    });
                }
                self.states.remove(&key);
                continue;
            }
            let state = self.states.entry(key.clone()).or_default();
            let mode = match b.mode {
                ValueMode::Auto => match b.target.kind() {
                    ControlKind::Button => ValueMode::Button,
                    ControlKind::Momentary => ValueMode::Momentary,
                    ControlKind::Continuous => ValueMode::Absolute,
                    ControlKind::Encoder => ValueMode::TwosComplement,
                },
                m => m,
            };
            let previous = state.raw.replace(raw);
            let value = match mode {
                ValueMode::Button | ValueMode::Momentary | ValueMode::Toggle => {
                    let down = raw != 0.;
                    if down == state.down {
                        continue;
                    }
                    state.down = down;
                    if mode == ValueMode::Toggle && b.target.kind() != ControlKind::Button {
                        if !down {
                            continue;
                        }
                        state.latched = !state.latched;
                        if b.target.kind() == ControlKind::Momentary {
                            if state.latched {
                                self.held.insert(key, b.target.clone());
                            } else {
                                self.held.remove(&key);
                            }
                            if state.latched {
                                MidiValue::Press
                            } else {
                                MidiValue::Release
                            }
                        } else {
                            MidiValue::Normalized(if state.latched { 1. } else { 0. })
                        }
                    } else if mode == ValueMode::Momentary {
                        if down {
                            self.held.insert(key, b.target.clone());
                        } else {
                            self.held.remove(&key);
                        }
                        if down {
                            if matches!(
                                b.target,
                                MidiTarget::TemporaryCue { .. } | MidiTarget::Cue { .. }
                            ) {
                                MidiValue::Hold
                            } else {
                                MidiValue::Press
                            }
                        } else {
                            MidiValue::Release
                        }
                    } else {
                        if !down {
                            continue;
                        }
                        MidiValue::Press
                    }
                }
                ValueMode::Absolute => {
                    if hid && previous == Some(raw) {
                        continue;
                    }
                    let min = b.minimum.unwrap_or(min);
                    let max = b.maximum.unwrap_or(max);
                    if max <= min {
                        continue;
                    }
                    let normalize = |v: f64| {
                        let v = ((v - min) / (max - min)).clamp(0., 1.);
                        if b.inverted {
                            1. - v
                        } else {
                            v
                        }
                    };
                    let normalized = normalize(raw);
                    if b.pickup {
                        let software = target_value(&b.target, snapshot);
                        if state
                            .last_software
                            .is_some_and(|last| (last - software).abs() > 0.02)
                        {
                            state.picked = false;
                        }
                        if !state.picked {
                            state.picked = (normalized - software).abs() <= 0.01
                                || previous.is_some_and(|p| {
                                    (normalize(p) - software) * (normalized - software) <= 0.
                                });
                        }
                        state.last_software =
                            Some(if state.picked { normalized } else { software });
                        if !state.picked {
                            continue;
                        }
                    }
                    // Preserve legacy 7-bit API semantics when no transformation is necessary.
                    if max == 127.
                        && min == 0.
                        && !b.pickup
                        && b.minimum.is_none()
                        && b.maximum.is_none()
                    {
                        MidiValue::Absolute((normalized * 127.).round() as u8)
                    } else {
                        MidiValue::Normalized(normalized)
                    }
                }
                ValueMode::TwosComplement
                | ValueMode::BinaryOffset
                | ValueMode::SignedBit
                | ValueMode::Counter => {
                    let range = max - min + 1.;
                    let half = range / 2.;
                    let v = raw - min;
                    let delta = match mode {
                        ValueMode::Counter => {
                            let Some(p) = previous else {
                                continue;
                            };
                            let mut d = raw - p;
                            if d > half {
                                d -= range;
                            }
                            if d < -half {
                                d += range;
                            }
                            d
                        }
                        ValueMode::BinaryOffset => v - half,
                        ValueMode::SignedBit => {
                            if v < half {
                                v
                            } else {
                                -(v - half)
                            }
                        }
                        _ if min < 0. => raw,
                        _ => {
                            if v == half {
                                0.
                            } else if v < half {
                                v
                            } else {
                                v - range
                            }
                        }
                    } * b.sensitivity
                        * if b.inverted { -1. } else { 1. };
                    if delta == 0. {
                        continue;
                    }
                    if let MidiTarget::Jog { deck } = b.target {
                        MidiValue::JogFrames(
                            delta * snapshot.deck(deck).src_sample_rate as f64 * 1.8
                                / b.ticks_per_revolution.unwrap_or(720.),
                        )
                    } else {
                        // Sub-unit sensitivity must accumulate across reports;
                        // rounding each pulse makes slow encoders stop moving.
                        let total = state.relative_remainder + delta;
                        let steps = total.trunc();
                        state.relative_remainder = total - steps;
                        if steps == 0. {
                            continue;
                        }
                        MidiValue::Relative(steps.clamp(i16::MIN as f64, i16::MAX as f64) as i16)
                    }
                }
                ValueMode::Auto => unreachable!(),
            };
            if value == MidiValue::Release && self.is_held(&b.target) {
                continue;
            }
            result.push(MidiAction {
                target: b.target.clone(),
                value,
            });
        }
        for b in &preset.bindings {
            if b.layer.as_ref().is_some_and(|l| {
                self.layers
                    .get(&(scope.into(), l.name.clone()))
                    .copied()
                    .unwrap_or(false)
                    != l.active
            }) {
                let binding_id = format!("{}:{}", preset.id, b.id);
                let keys: Vec<_> = self
                    .states
                    .keys()
                    .filter(|(port, bid)| {
                        bid == &binding_id && self.scopes.get(port).is_some_and(|s| s == scope)
                    })
                    .cloned()
                    .collect();
                for key in keys {
                    if let Some(target) = self.held.remove(&key) {
                        result.push(MidiAction {
                            target,
                            value: MidiValue::Release,
                        });
                    }
                    self.states.remove(&key);
                }
            }
        }
        result.retain(|a| a.value != MidiValue::Release || !self.is_held(&a.target));
        result
    }
}

pub(crate) fn target_value(target: &MidiTarget, s: &EngineSnapshot) -> f64 {
    use MidiTarget::*;
    let d = target.deck().map(|d| s.deck(d));
    let value = match target {
        Xfader => (s.xfader + 1.) / 2.,
        Master => s.master,
        MasterGain => (s.master_gain_db + 12.) / 24.,
        CueMix => s.cue_mix,
        CueGain => s.cue_gain,
        Quantize => u8::from(s.quantize) as f32,
        XfReverse => u8::from(s.xf_reverse) as f32,
        Automix => u8::from(s.automix_on) as f32,
        Fader { .. } => d.unwrap().fader,
        Gain { .. } => (d.unwrap().gain_db + 12.) / 24.,
        Balance { .. } => (d.unwrap().balance + 1.) / 2.,
        Filter { .. } => (d.unwrap().filter_amount + 1.) / 2.,
        Tempo { .. } => (d.unwrap().rate - 0.88) / 0.24,
        Pitch { .. } => (d.unwrap().pitch_semitones + 6.) / 12.,
        Play { .. } => u8::from(d.unwrap().playing) as f32,
        Sync { .. } => u8::from(d.unwrap().synced) as f32,
        KeyLock { .. } => u8::from(d.unwrap().keylock) as f32,
        Pfl { .. } => u8::from(d.unwrap().pfl) as f32,
        Vinyl { .. } => u8::from(d.unwrap().vinyl) as f32,
        Slip { .. } => u8::from(d.unwrap().slip) as f32,
        Reverse { .. } => u8::from(d.unwrap().reverse) as f32,
        Loop { .. } => u8::from(d.unwrap().loop_on) as f32,
        Eq { band, .. } => (d.unwrap().eq_db[band.index()] + 12.) / 24.,
        EqKill { band, .. } => u8::from(d.unwrap().eq_kill[band.index()]) as f32,
        Cue { index, .. } => u8::from(
            d.unwrap()
                .cues
                .get(*index as usize)
                .is_some_and(|v| v.is_some()),
        ) as f32,
        Stem { stem, .. } => d.unwrap().stem_gain[stem.index()],
        TemporaryCue { .. } => u8::from(d.unwrap().temporary_cue_frame.is_some()) as f32,
        Brake { .. } => u8::from(d.unwrap().brake) as f32,
        Resonance { .. } => d.unwrap().filter_resonance,
        ResonanceEnabled { .. } => u8::from(d.unwrap().filter_resonance_enabled) as f32,
        DeckGain { .. } => (d.unwrap().effective_limiter_gain_db() + 12.) / 24.,
        FxOn { slot, .. } => {
            u8::from(d.unwrap().fx.get(*slot as usize).is_some_and(|f| f.on)) as f32
        }
        FxSend { .. } => d.unwrap().send,
        FxMix { slot, .. } => d.unwrap().fx.get(*slot as usize).map_or(0., |f| f.mix),
        _ => 0.,
    };
    value.clamp(0., 1.) as f64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fields_cross_bytes_and_preserve_neighbors() {
        for little in [true, false] {
            let mut report = [0x02, 0xa5, 0x5a, 0xff];
            let before = report;
            assert!(write_report_field(&mut report, 11, 12, little, 0xabc));
            assert_eq!(read_hid_field(&report, 11, 12, little, false), Some(0xabc));
            assert_eq!(report[0], before[0]);
            assert_eq!(report[3], before[3]);
        }
        assert_eq!(read_hid_field(&[0, 0xff], 8, 8, true, true), Some(-1));
    }
    #[test]
    fn fractional_relative_sensitivity_accumulates_and_resets_per_device() {
        for (mode, forward, backward) in [
            (MidiControlMode::RelativeTwosComplement, 1, 127),
            (MidiControlMode::RelativeBinaryOffset, 65, 63),
            (MidiControlMode::RelativeSignedBit, 1, 65),
        ] {
            let mut p = ControllerPreset::blank("Fractional encoder fixture");
            p.bindings.push(Binding::from_legacy(&MidiBinding {
                address: None,
                inverted: false,
                sensitivity: 0.25,
                ticks_per_revolution: None,
                pickup: false,
                device_id: None,
                kind: MidiSourceKind::Cc,
                channel: 0,
                number: 1,
                target: MidiTarget::BrowseTracks,
                mode,
            }));
            p.validate().unwrap();
            let mut router = Router::default();
            let snapshot = EngineSnapshot::default();
            for value in [forward, backward] {
                let actions: Vec<_> = (0..4)
                    .flat_map(|_| {
                        router.route("one", "main", &p, false, &[0xb0, 1, value], &snapshot)
                    })
                    .collect();
                assert_eq!(actions.len(), 1);
                assert_eq!(
                    actions[0].value,
                    MidiValue::Relative(if value == forward { 1 } else { -1 })
                );
            }
            assert!(router
                .route("one", "main", &p, false, &[0xb0, 1, forward], &snapshot)
                .is_empty());
            router.release_device("one");
            // Reconnection and another physical input never inherit a partial step.
            for device in ["one", "two"] {
                for _ in 0..3 {
                    assert!(router
                        .route(device, "main", &p, false, &[0xb0, 1, forward], &snapshot)
                        .is_empty());
                }
                assert_eq!(
                    router.route(device, "main", &p, false, &[0xb0, 1, forward], &snapshot)[0]
                        .value,
                    MidiValue::Relative(1)
                );
            }
        }
    }

    #[test]
    fn momentary_disconnect_and_counter_wrap() {
        let mut p = ControllerPreset::blank("test");
        p.bindings.push(Binding::from_legacy(&MidiBinding {
            address: None,
            inverted: false,
            sensitivity: 1.,
            ticks_per_revolution: None,
            pickup: false,
            device_id: None,
            kind: MidiSourceKind::Note,
            channel: 0,
            number: 4,
            target: MidiTarget::JogTouch {
                deck: mixless_protocol::DeckId::A,
            },
            mode: MidiControlMode::Auto,
        }));
        let mut r = Router::default();
        let s = EngineSnapshot::default();
        assert_eq!(
            r.route("d", "main", &p, false, &[0x90, 4, 127], &s).len(),
            1
        );
        assert!(r
            .route("d", "main", &p, false, &[0x90, 4, 127], &s)
            .is_empty());
        assert_eq!(r.release_device("d")[0].value, MidiValue::Release);
        assert!(r.release_device("d").is_empty());
    }
}
