use super::*;
use crate::replay::{redact_snapshot, ReplayFrame, ReportTrace, MAX_TRACE_FRAMES};

pub(crate) struct Recorder {
    pub device_id: String,
    pub trace: ReportTrace,
    origin_ms: Option<u64>,
    last_state: Option<mixless_protocol::EngineSnapshot>,
    bytes: usize,
    connected: bool,
}
impl Recorder {
    pub(crate) fn new(device_id: String, trace: ReportTrace) -> Self {
        Self {
            device_id,
            trace,
            origin_ms: None,
            last_state: None,
            bytes: 0,
            connected: false,
        }
    }
}
impl Worker {
    pub(super) fn truncate_recording(&self, id: &str) {
        if let Some(recorder) = self.shared.recording.lock().unwrap().as_mut() {
            if recorder.device_id == id {
                recorder.trace.truncated = true;
            }
        }
    }

    pub(super) fn record(
        &self,
        id: &str,
        event: AdapterEvent,
        snapshot: mixless_protocol::EngineSnapshot,
    ) {
        let mut guard = self.shared.recording.lock().unwrap();
        let Some(recorder) = guard
            .as_mut()
            .filter(|r| r.device_id == id && !r.trace.truncated)
        else {
            return;
        };
        // A capture must begin at a real session boundary, not invent an initialization that did not run.
        if recorder.origin_ms.is_none() && event.kind != EventKind::Connect {
            return;
        }
        if event.kind == EventKind::Connect {
            if self
                .profiles
                .iter()
                .find(|(a, _)| a.device_id == id)
                .is_none_or(|(_, p)| {
                    crate::community::content_digest(p) != recorder.trace.preset_digest
                })
            {
                recorder.trace.truncated = true;
                return;
            }
            if recorder.connected {
                return;
            }
            recorder.connected = true;
        } else if event.kind == EventKind::Disconnect {
            if !recorder.connected {
                return;
            }
            recorder.connected = false;
        } else if !recorder.connected {
            return;
        }
        recorder.origin_ms.get_or_insert(event.at_ms);
        let snapshot = redact_snapshot(snapshot);
        let state = (recorder.last_state.as_ref() != Some(&snapshot)).then_some(snapshot.clone());
        let frame = ReplayFrame {
            event,
            state,
            expected: None,
            expected_error: None,
        };
        let bytes = serde_json::to_vec(&frame).map_or(usize::MAX, |v| v.len());
        if recorder.trace.frames.len() >= MAX_TRACE_FRAMES
            || recorder.bytes.saturating_add(bytes) > 8 * 1024 * 1024
        {
            recorder.trace.truncated = true;
            return;
        }
        recorder.last_state = Some(snapshot);
        recorder.bytes += bytes;
        recorder.trace.frames.push(frame);
    }
}
