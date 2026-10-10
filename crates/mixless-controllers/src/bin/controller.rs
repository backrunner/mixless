//! Developer bench. Replay and validation are entirely offline; only `record` opens selected inputs.
#[path = "controller_tool/schema.rs"]
mod schema;
use mixless_controllers::{adapter::AdapterInstance, community::content_digest, replay::*, *};
use std::{collections::BTreeMap, fs, path::Path, time::Duration};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("schema") if args.len() == 1 => print_json(&schema::preset_schema()),
        Some("targets") if args.len() == 1 => print_json(&MidiTarget::all()),
        Some("fingerprint") if args.len() == 2 => { println!("{}", content_digest(&read_preset(Path::new(&args[1]))?)); Ok(()) }
        Some("validate") if args.len() == 2 => {
            let p = read_preset(Path::new(&args[1]))?;
            validate_adapter(&p)?;
            println!("Valid: {}\nContent: {}", p.name, content_digest(&p)); Ok(())
        }
        Some("pack") if args.len() == 4 => {
            let mut p: ControllerPreset = serde_json::from_slice(&fs::read(&args[1])?)?;
            let module = wat::parse_file(&args[2])?;
            p.adapter.as_mut().ok_or("Template has no adapter definition")?.set_module(&module);
            p.validate()?; validate_adapter(&p)?;
            write_new(Path::new(&args[3]), &p)
        }
        Some("pin-trace") if args.len() == 4 => {
            let p = read_preset(Path::new(&args[1]))?;
            let mut trace = read_trace(Path::new(&args[2]))?;
            trace.preset_digest = content_digest(&p);
            replay(&p, &trace, true)?; // Pin only after the independently written expectations pass.
            write_new(Path::new(&args[3]), &trace)
        }
        Some("replay") if args.len() == 3 || args.len() == 4 && args[3] == "--check" => {
            let p = read_preset(Path::new(&args[1]))?;
            let trace = read_trace(Path::new(&args[2]))?;
            print_json(&replay(&p, &trace, args.len() == 4)?)
        }
        Some("devices") if args.len() == 1 => {
            let temp = tempfile::tempdir()?;
            let manager = ControllerManager::start_with_config(temp.path().join("midi.json"), MidiConfig { enabled: false, input_ids: None })?;
            for d in manager.devices() { println!("{}\t{}\t{}", if d.hid { "HID" } else { "MIDI" }, d.name, d.id); }
            Ok(())
        }
        Some("record") if args.len() == 7 => record(&args[1..]),
        _ => Err("Usage: mixless-controller schema | targets | devices\n  validate PRESET\n  fingerprint PRESET\n  pack TEMPLATE WASM_OR_WAT OUTPUT\n  pin-trace PRESET TRACE_TEMPLATE OUTPUT\n  replay PRESET TRACE [--check]\n  record PRESET PORTS_JSON OUTPUT SECONDS FIRMWARE OS\nPORTS_JSON maps each logical endpoint to the exact ID printed by devices. Record applies the preset and sends its initialization and feedback reports.".into()),
    }
}
fn validate_adapter(p: &ControllerPreset) -> Result {
    if p.adapter.is_some() {
        AdapterInstance::new(p)?;
    }
    Ok(())
}
fn print_json(v: &impl serde::Serialize) -> Result {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}
fn write_new(path: &Path, v: &impl serde::Serialize) -> Result {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(v)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)?;
    println!("Saved {}", path.display());
    Ok(())
}
fn record(args: &[String]) -> Result {
    let p = read_preset(Path::new(&args[0]))?;
    validate_adapter(&p)?;
    let ports: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&args[1])?)?;
    let seconds: u64 = args[3].parse()?;
    if !(1..=600).contains(&seconds) {
        return Err("Duration must be 1–600 seconds".into());
    }
    if Path::new(&args[2]).exists() {
        return Err("Capture output already exists".into());
    }
    let primary = ports
        .values()
        .next()
        .ok_or("Choose an input for every endpoint")?
        .clone();
    let temp = tempfile::tempdir()?;
    let manager = ControllerManager::start_with_config(
        temp.path().join("midi.json"),
        MidiConfig {
            enabled: false,
            input_ids: None,
        },
    )?;
    let devices = manager.devices();
    if ports
        .values()
        .any(|id| !devices.iter().any(|d| &d.id == id))
    {
        return Err("A selected device is unavailable".into());
    }
    let id = manager.save_preset(p)?;
    manager.start_recording(&primary, &id, &args[4], &args[5])?;
    manager.assign(DeviceAssignment {
        device_id: primary.clone(),
        preset_id: id,
        ports: ports.clone(),
        outputs: BTreeMap::new(),
    })?;
    manager.configure(MidiConfig {
        enabled: true,
        input_ids: Some(ports.into_values().collect()),
    })?;
    println!("Recording for {seconds} seconds. No audio playback is started.");
    std::thread::sleep(Duration::from_secs(seconds));
    let fault = manager.feedback_error(&primary);
    manager.unassign(&primary)?;
    let trace = manager.stop_recording()?;
    if trace.frames.is_empty() {
        return Err("No session was captured; check the device assignment".into());
    }
    write_new(Path::new(&args[2]), &trace)?;
    if let Some(e) = fault {
        eprintln!("Controller reported: {e}");
    }
    if trace.truncated {
        return Err("Capture limit reached; saved incomplete trace for inspection".into());
    }
    println!(
        "Captured {} frames. Review replay output before adding fixture expectations.",
        trace.frames.len()
    );
    Ok(())
}
