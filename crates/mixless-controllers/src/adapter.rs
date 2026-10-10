//! Independent Wasm ABI. The interpreter never receives a device handle or ambient capabilities.
mod host;
#[cfg(test)]
pub(crate) mod tests;

use crate::{
    community::AdapterDefinition, error, ControllerPreset, MidiAction, MidiError, MidiTarget,
    MidiValue, OutputPacket,
};
use mixless_protocol::EngineSnapshot;
use serde::{Deserialize, Serialize};
use wasmi::{
    Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

pub const FUEL_PER_EVENT: u64 = 50_000;
pub const MAX_EFFECTS: usize = 64;
pub const MAX_MEMORY: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum EventKind {
    Connect = 0,
    Input = 1,
    State = 2,
    Timer = 3,
    Disconnect = 4,
    Feature = 5,
    IoError = 6,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterEvent {
    pub kind: EventKind,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub data: Vec<u8>,
    pub at_ms: u64,
}

/// Only declared control values and meters cross the ABI. No titles, paths or library identifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterState {
    pub values: Vec<f64>,
    pub meters: [[f32; 2]; 3],
    pub sample_rates: [u32; 2],
}
impl AdapterState {
    pub fn from_snapshot(definition: &AdapterDefinition, s: &EngineSnapshot) -> Self {
        Self {
            values: definition
                .controls
                .iter()
                .map(|t| crate::router::target_value(t, s))
                .collect(),
            meters: [s.master_level, s.decks[0].level, s.decks[1].level],
            sample_rates: [s.decks[0].src_sample_rate, s.decks[1].src_sample_rate],
        }
    }
    pub fn validate(&self, count: usize) -> Result<(), MidiError> {
        if self.values.len() != count
            || self
                .values
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || self.meters.iter().flatten().any(|v| !v.is_finite())
            || self.sample_rates.iter().any(|v| *v > 768_000)
        {
            return Err(error("Invalid adapter state"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdapterEffect {
    Action {
        action: MidiAction,
    },
    Write {
        packet: OutputPacket,
    },
    ReadFeature {
        endpoint: String,
        report_id: u8,
        length: u16,
    },
    Timer {
        delay_ms: u32,
    },
}

struct Host {
    limits: StoreLimits,
    definition: AdapterDefinition,
    endpoints: Vec<String>,
    state: AdapterState,
    effects: Vec<AdapterEffect>,
    active: bool,
}
impl Host {
    fn push(&mut self, effect: AdapterEffect) -> Result<(), wasmi::Error> {
        if !self.active || self.effects.len() >= MAX_EFFECTS {
            return Err(wasmi::Error::new("Adapter effect budget exceeded"));
        }
        self.effects.push(effect);
        Ok(())
    }
}

pub struct AdapterInstance {
    store: Store<Host>,
    memory: Memory,
    callback: TypedFunc<(i32, i32, i32, i32, i64), i32>,
    buffer: usize,
    held: Vec<MidiTarget>,
    failed: bool,
    last_ms: u64,
    window_start: u64,
    events_in_window: usize,
    outputs_in_window: usize,
}

impl AdapterInstance {
    /// Validates and instantiates with no I/O. Used by import preview, replay and device workers.
    pub fn new(p: &ControllerPreset) -> Result<Self, MidiError> {
        p.validate()?;
        let definition = p
            .adapter
            .as_ref()
            .ok_or_else(|| error("Preset has no adapter"))?;
        let mut config = Config::default();
        config
            .consume_fuel(true)
            .allow_start_fn(false)
            .set_max_recursion_depth(64)
            .set_max_stack_height(16 * 1024)
            .enforced_limits(EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let module = Module::new(&engine, definition.module_bytes()?).map_err(error)?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(MAX_MEMORY)
            .memories(1)
            .instances(1)
            .tables(1)
            .table_elements(1024)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(
            &engine,
            Host {
                limits,
                definition: definition.clone(),
                endpoints: p.endpoints.iter().map(|e| e.id.clone()).collect(),
                state: AdapterState::from_snapshot(definition, &EngineSnapshot::default()),
                effects: vec![],
                active: false,
            },
        );
        store.limiter(|h| &mut h.limits);
        store.set_fuel(FUEL_PER_EVENT).map_err(error)?;
        let mut linker = Linker::new(&engine);
        host::link(&mut linker).map_err(error)?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(error)?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| error("Adapter must export memory"))?;
        let buffer = instance
            .get_typed_func::<(), i32>(&store, "mixless_buffer")
            .map_err(error)?
            .call(&mut store, ())
            .map_err(error)?;
        if buffer < 0 || buffer as usize + 4096 > memory.data_size(&store) {
            return Err(error("Adapter must reserve a 4096-byte input buffer"));
        }
        let callback = instance
            .get_typed_func(&store, "mixless_event")
            .map_err(error)?;
        Ok(Self {
            store,
            memory,
            callback,
            buffer: buffer as usize,
            held: vec![],
            failed: false,
            last_ms: 0,
            window_start: 0,
            events_in_window: 0,
            outputs_in_window: 0,
        })
    }

    pub fn process(
        &mut self,
        event: &AdapterEvent,
        state: &AdapterState,
    ) -> Result<Vec<AdapterEffect>, MidiError> {
        if self.failed {
            return Err(error("Adapter stopped; reconnect or reapply the preset"));
        }
        let result = self.process_inner(event, state);
        if result.is_err() {
            self.failed = true;
            self.store.data_mut().effects.clear();
        }
        result
    }

    fn process_inner(
        &mut self,
        event: &AdapterEvent,
        state: &AdapterState,
    ) -> Result<Vec<AdapterEffect>, MidiError> {
        state.validate(self.store.data().definition.controls.len())?;
        if event.data.len() > 4096 || event.at_ms < self.last_ms || event.at_ms > i64::MAX as u64 {
            return Err(error("Invalid adapter event"));
        }
        self.last_ms = event.at_ms;
        if event.at_ms.saturating_sub(self.window_start) >= 1000 {
            self.window_start = event.at_ms;
            self.events_in_window = 0;
            self.outputs_in_window = 0;
        }
        self.events_in_window += 1;
        if self.events_in_window > 4096 {
            return Err(error("Adapter input rate exceeded"));
        }
        let endpoint = if event.endpoint.is_empty() {
            -1
        } else {
            self.store
                .data()
                .endpoints
                .iter()
                .position(|e| e == &event.endpoint)
                .ok_or_else(|| error("Unknown adapter endpoint"))? as i32
        };
        if matches!(
            event.kind,
            EventKind::Input | EventKind::Feature | EventKind::IoError
        ) && endpoint < 0
        {
            return Err(error("Report event needs an endpoint"));
        }
        self.store.set_fuel(FUEL_PER_EVENT).map_err(error)?;
        self.memory
            .write(&mut self.store, self.buffer, &event.data)
            .map_err(error)?;
        {
            let h = self.store.data_mut();
            h.state = state.clone();
            h.effects.clear();
            h.active = true;
        }
        let outcome = self.callback.call(
            &mut self.store,
            (
                event.kind as i32,
                endpoint,
                self.buffer as i32,
                event.data.len() as i32,
                event.at_ms as i64,
            ),
        );
        self.store.data_mut().active = false;
        let status = outcome.map_err(error)?;
        if status != 0 {
            return Err(error(format!("Adapter returned error {status}")));
        }
        let effects = std::mem::take(&mut self.store.data_mut().effects);
        self.outputs_in_window += effects
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    AdapterEffect::Write { .. } | AdapterEffect::ReadFeature { .. }
                )
            })
            .count();
        if self.outputs_in_window > 256 {
            return Err(error("Adapter output rate exceeded"));
        }
        for effect in &effects {
            if let AdapterEffect::Action { action } = effect {
                if matches!(action.value, MidiValue::Hold) && !self.held.contains(&action.target) {
                    self.held.push(action.target.clone());
                } else if action.value == MidiValue::Release {
                    self.held.retain(|t| t != &action.target);
                }
            }
        }
        Ok(effects)
    }

    pub fn release_held(&mut self) -> Vec<AdapterEffect> {
        self.held
            .drain(..)
            .map(|target| AdapterEffect::Action {
                action: MidiAction {
                    target,
                    value: MidiValue::Release,
                },
            })
            .collect()
    }
}
