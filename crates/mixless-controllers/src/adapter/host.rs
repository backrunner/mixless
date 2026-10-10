use super::*;
use crate::{community::ReportAccess, ControlKind, ReportKind};
use wasmi::{Caller, Error};

fn permission(
    h: &Host,
    endpoint: i32,
    access: ReportAccess,
    report: u8,
    length: usize,
) -> Result<String, Error> {
    let name = h
        .endpoints
        .get(endpoint as usize)
        .ok_or_else(|| Error::new("Unknown endpoint index"))?;
    if !h.definition.reports.iter().any(|p| {
        p.endpoint == *name
            && p.access == access
            && p.report_id == report
            && p.length as usize == length
    }) {
        return Err(Error::new("Report is not permitted by the preset"));
    }
    Ok(name.clone())
}

pub(super) fn link(linker: &mut Linker<Host>) -> Result<(), Error> {
    linker.func_wrap(
        "mixless",
        "emit",
        |mut c: Caller<'_, Host>, index: i32, kind: i32, value: f64| -> Result<(), Error> {
            let target = c
                .data()
                .definition
                .controls
                .get(index as usize)
                .cloned()
                .ok_or_else(|| Error::new("Unknown control index"))?;
            if !value.is_finite() {
                return Err(Error::new("Control value must be finite"));
            }
            let hold = matches!(
                target,
                MidiTarget::JogTouch { .. }
                    | MidiTarget::Brake { .. }
                    | MidiTarget::TemporaryCue { .. }
                    | MidiTarget::Cue { .. }
            );
            let value = match kind {
                0 if target.kind() == ControlKind::Button => MidiValue::Press,
                1 if hold => MidiValue::Hold,
                2 if hold => MidiValue::Release,
                3 if target.kind() == ControlKind::Continuous && (0.0..=1.0).contains(&value) => {
                    MidiValue::Normalized(value)
                }
                4 if matches!(
                    target.kind(),
                    ControlKind::Continuous | ControlKind::Encoder
                ) && !matches!(target, MidiTarget::Jog { .. })
                    && value.fract() == 0.0
                    && value.abs() <= 4096.0 =>
                {
                    MidiValue::Relative(value as i16)
                }
                // A platter turn corresponds to 1.8 source seconds, as in declarative jog bindings.
                5 if matches!(target, MidiTarget::Jog { .. }) && value.abs() <= 1.0 => {
                    let deck = target.deck().unwrap();
                    MidiValue::JogFrames(
                        value * c.data().state.sample_rates[deck.index()] as f64 * 1.8,
                    )
                }
                _ => return Err(Error::new("Value kind or range does not match the control")),
            };
            c.data_mut().push(AdapterEffect::Action {
                action: MidiAction { target, value },
            })
        },
    )?;
    linker.func_wrap(
        "mixless",
        "value",
        |c: Caller<'_, Host>, index: i32| -> Result<f64, Error> {
            c.data()
                .state
                .values
                .get(index as usize)
                .copied()
                .ok_or_else(|| Error::new("Unknown control index"))
        },
    )?;
    linker.func_wrap(
        "mixless",
        "meter",
        |c: Caller<'_, Host>, bus: i32, channel: i32| -> Result<f64, Error> {
            c.data()
                .state
                .meters
                .get(bus as usize)
                .and_then(|v| v.get(channel as usize))
                .map(|v| v.clamp(0.0, 1.0) as f64)
                .ok_or_else(|| Error::new("Unknown meter"))
        },
    )?;
    linker.func_wrap(
        "mixless",
        "send",
        |mut c: Caller<'_, Host>,
         endpoint: i32,
         kind: i32,
         ptr: i32,
         len: i32|
         -> Result<(), Error> {
            if ptr < 0 || !(1..=1024).contains(&len) {
                return Err(Error::new("Invalid output buffer"));
            }
            let memory = c
                .get_export("memory")
                .and_then(|e| e.into_memory())
                .ok_or_else(|| Error::new("Missing memory"))?;
            let mut bytes = vec![0; len as usize];
            memory
                .read(&c, ptr as usize, &mut bytes)
                .map_err(|e| Error::new(e.to_string()))?;
            let (access, report) = match kind {
                0 => (ReportAccess::Output, ReportKind::Output),
                1 => (ReportAccess::FeatureWrite, ReportKind::Feature),
                _ => return Err(Error::new("Unknown report kind")),
            };
            let endpoint = permission(c.data(), endpoint, access, bytes[0], bytes.len())?;
            c.data_mut().push(AdapterEffect::Write {
                packet: OutputPacket {
                    endpoint,
                    data: bytes,
                    report: Some(report),
                },
            })
        },
    )?;
    linker.func_wrap(
        "mixless",
        "read_feature",
        |mut c: Caller<'_, Host>, endpoint: i32, report: i32, length: i32| -> Result<(), Error> {
            if !(0..=255).contains(&report) || !(1..=1024).contains(&length) {
                return Err(Error::new("Invalid feature request"));
            }
            let endpoint = permission(
                c.data(),
                endpoint,
                ReportAccess::FeatureRead,
                report as u8,
                length as usize,
            )?;
            c.data_mut().push(AdapterEffect::ReadFeature {
                endpoint,
                report_id: report as u8,
                length: length as u16,
            })
        },
    )?;
    linker.func_wrap(
        "mixless",
        "timer",
        |mut c: Caller<'_, Host>, delay: i32| -> Result<(), Error> {
            if !(10..=60_000).contains(&delay)
                || c.data()
                    .effects
                    .iter()
                    .any(|e| matches!(e, AdapterEffect::Timer { .. }))
            {
                return Err(Error::new(
                    "One timer per callback, between 10 and 60000 ms",
                ));
            }
            c.data_mut().push(AdapterEffect::Timer {
                delay_ms: delay as u32,
            })
        },
    )?;
    Ok(())
}
