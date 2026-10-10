//! Portable report captures and deterministic replay. No device is opened during replay.
use crate::{adapter::*, community::content_digest, error, *};
use mixless_protocol::EngineSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
mod tests;

pub const MAX_TRACE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TRACE_FRAMES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportTrace {
    pub version: u32,
    pub preset_digest: String,
    pub model: String,
    pub firmware: String,
    pub os: String,
    #[serde(default)]
    pub truncated: bool,
    pub frames: Vec<ReplayFrame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayFrame {
    pub event: AdapterEvent,
    /// Captures redact library and local device metadata. Omitted snapshots reuse the previous state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<EngineSnapshot>,
    /// None means observation only. A fixture must explicitly assert even an empty effects list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Vec<AdapterEffect>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayResult {
    pub frames: usize,
    pub assertions: usize,
    pub outputs: Vec<Vec<AdapterEffect>>,
    pub errors: Vec<Option<String>>,
}

pub fn read_trace(path: &std::path::Path) -> Result<ReportTrace, MidiError> {
    crate::read_document(path, MAX_TRACE_BYTES)
}

pub fn replay(
    p: &ControllerPreset,
    trace: &ReportTrace,
    require_assertions: bool,
) -> Result<ReplayResult, MidiError> {
    p.validate()?;
    if trace.version != 1
        || trace.truncated
        || trace.frames.is_empty()
        || trace.frames.len() > MAX_TRACE_FRAMES
    {
        return Err(error("Unsupported, empty or incomplete trace"));
    }
    if trace.preset_digest != content_digest(p) {
        return Err(error("Trace belongs to different preset content"));
    }
    let mut session = ReplaySession::new(p.clone());
    let mut snapshot = EngineSnapshot::default();
    let mut result = ReplayResult {
        frames: trace.frames.len(),
        assertions: 0,
        outputs: vec![],
        errors: vec![],
    };
    let mut last_ms = 0;
    for (i, frame) in trace.frames.iter().enumerate() {
        if frame.event.at_ms < last_ms {
            return Err(error(format!("Frame {i}: timestamps must be ordered")));
        }
        last_ms = frame.event.at_ms;
        if let Some(s) = &frame.state {
            snapshot = s.clone();
        }
        if require_assertions && frame.expected.is_none() && frame.expected_error.is_none() {
            return Err(error(format!(
                "Frame {i} has no expectation; review replay output before making a fixture"
            )));
        }
        let (effects, failure) = session.process(&frame.event, &snapshot);
        if frame
            .expected_error
            .as_ref()
            .is_some_and(|e| e.is_empty() || !failure.as_ref().is_some_and(|f| f.contains(e)))
            || failure.is_some() && frame.expected_error.is_none()
        {
            return Err(error(format!(
                "Frame {i}: unexpected adapter result: {failure:?}"
            )));
        }
        if let Some(expected) = &frame.expected {
            if *expected != effects {
                return Err(error(format!(
                    "Frame {i}: effects differ\nexpected: {expected:?}\nactual: {effects:?}"
                )));
            }
        }
        result.assertions +=
            usize::from(frame.expected.is_some() || frame.expected_error.is_some());
        result.outputs.push(effects);
        result.errors.push(failure);
    }
    if session.connected {
        return Err(error("Trace must end with a disconnect"));
    }
    Ok(result)
}

struct ReplaySession {
    preset: ControllerPreset,
    adapter: Option<AdapterInstance>,
    router: crate::router::Router,
    feedback: HashMap<usize, Vec<u8>>,
    connected: bool,
    faulted: bool,
    timer: Option<u64>,
}
impl ReplaySession {
    fn new(preset: ControllerPreset) -> Self {
        Self {
            preset,
            adapter: None,
            router: Default::default(),
            feedback: HashMap::new(),
            connected: false,
            faulted: false,
            timer: None,
        }
    }
    fn process(
        &mut self,
        event: &AdapterEvent,
        snapshot: &EngineSnapshot,
    ) -> (Vec<AdapterEffect>, Option<String>) {
        let result = self.event(event, snapshot);
        match result {
            Ok(effects) => (effects, None),
            Err(e) => {
                self.faulted = true;
                self.timer = None;
                let releases = self
                    .adapter
                    .as_mut()
                    .map_or_else(Vec::new, AdapterInstance::release_held);
                let mut releases = releases;
                releases.extend(
                    self.router
                        .release_all()
                        .into_iter()
                        .map(|(_, action)| AdapterEffect::Action { action }),
                );
                (releases, Some(e.to_string()))
            }
        }
    }
    fn event(
        &mut self,
        event: &AdapterEvent,
        snapshot: &EngineSnapshot,
    ) -> Result<Vec<AdapterEffect>, MidiError> {
        if event.kind == EventKind::Connect {
            if self.connected {
                return Err(error("Duplicate connect"));
            }
            self.adapter = self
                .preset
                .adapter
                .as_ref()
                .map(|_| AdapterInstance::new(&self.preset))
                .transpose()?;
            self.connected = true;
            self.faulted = false;
        } else if !self.connected {
            return Err(error("Report before connect"));
        }
        if event.kind == EventKind::Timer {
            if self.timer.is_none_or(|due| event.at_ms < due) {
                return Err(error("Timer fired before its deadline"));
            }
            self.timer = None;
        }
        if event.kind == EventKind::Disconnect {
            self.connected = false;
            self.timer = None;
        }
        if self.faulted {
            return Ok(vec![]);
        }
        if event.kind == EventKind::Disconnect {
            if let Some(a) = &mut self.adapter {
                let releases = a.release_held();
                // Match retirement in the live host: guest cleanup effects are advisory and discarded.
                let _ = a.process(
                    event,
                    &AdapterState::from_snapshot(self.preset.adapter.as_ref().unwrap(), snapshot),
                );
                return Ok(releases);
            }
        }
        let mut effects = if let Some(a) = &mut self.adapter {
            a.process(
                event,
                &AdapterState::from_snapshot(self.preset.adapter.as_ref().unwrap(), snapshot),
            )?
        } else {
            self.declarative(event, snapshot)?
        };
        for effect in &effects {
            if let AdapterEffect::Timer { delay_ms } = effect {
                self.timer = Some(event.at_ms.saturating_add(*delay_ms as u64));
            }
        }
        if event.kind == EventKind::Disconnect {
            if let Some(a) = &mut self.adapter {
                effects.extend(a.release_held());
            }
            self.timer = None;
        }
        Ok(effects)
    }
    fn declarative(
        &mut self,
        event: &AdapterEvent,
        s: &EngineSnapshot,
    ) -> Result<Vec<AdapterEffect>, MidiError> {
        let actions = |actions: Vec<MidiAction>| {
            actions
                .into_iter()
                .map(|action| AdapterEffect::Action { action })
                .collect()
        };
        Ok(match event.kind {
            EventKind::Connect => {
                self.feedback.clear();
                self.preset
                    .initialize
                    .iter()
                    .cloned()
                    .map(|packet| AdapterEffect::Write { packet })
                    .collect()
            }
            EventKind::Input => {
                let endpoint = self
                    .preset
                    .endpoints
                    .iter()
                    .find(|e| e.id == event.endpoint)
                    .ok_or_else(|| error("Unknown endpoint"))?;
                actions(self.router.route_for_controller(
                    "replay",
                    &event.endpoint,
                    &event.endpoint,
                    &self.preset,
                    matches!(endpoint.matcher, DeviceMatcher::Hid { .. }),
                    &event.data,
                    s,
                ))
            }
            EventKind::State => self
                .preset
                .feedback
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let packet = feedback_packet(f, s)?;
                    if self.feedback.get(&i) == Some(&packet.data) {
                        return None;
                    }
                    self.feedback.insert(i, packet.data.clone());
                    Some(AdapterEffect::Write { packet })
                })
                .collect(),
            EventKind::Disconnect => {
                let mut effects: Vec<_> = self
                    .preset
                    .shutdown
                    .iter()
                    .cloned()
                    .map(|packet| AdapterEffect::Write { packet })
                    .collect();
                effects.extend(
                    self.router
                        .release_all()
                        .into_iter()
                        .map(|(_, action)| AdapterEffect::Action { action }),
                );
                effects
            }
            _ => vec![],
        })
    }
}

pub(crate) fn feedback_packet(f: &FeedbackRule, s: &EngineSnapshot) -> Option<OutputPacket> {
    let value = match &f.signal {
        FeedbackSignal::Control { target } => crate::router::target_value(target, s),
        FeedbackSignal::Meter { deck } => deck
            .map_or(s.master_level, |d| s.deck(d).level)
            .into_iter()
            .fold(0f32, f32::max)
            .clamp(0., 1.) as f64,
    };
    let mut packet = f.packet.clone();
    let value = (f.off as f64 + (f.on as f64 - f.off as f64) * value).round() as u32;
    crate::router::write_report_field(
        &mut packet.data,
        f.bit_offset,
        f.bits,
        f.little_endian,
        value,
    )
    .then_some(packet)
}

pub fn redact_snapshot(mut s: EngineSnapshot) -> EngineSnapshot {
    for deck in &mut s.decks {
        deck.track_id = None;
        deck.title = None;
        deck.artist = None;
    }
    s.device_name.clear();
    s.cue_device = None;
    s.master_output = None;
    s.cue_output = None;
    s
}
