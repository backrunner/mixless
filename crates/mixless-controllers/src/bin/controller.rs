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
        Some("presets") if args.len() == 1 => { for p in builtin_presets() { println!("{}\t{}", p.id, p.name); } Ok(()) },
        Some("export-builtin") if args.len() == 3 => {
            let p = builtin_presets().into_iter().find(|p| p.id == args[1]).ok_or("Unknown built-in preset")?;
            write_new(Path::new(&args[2]), &p)
        },
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
        Some("capture-input") if args.len() == 8 => capture_input(&args[1..]),
        _ => Err("Usage: mixless-controller schema | targets | presets | devices\n  export-builtin ID OUTPUT\n  validate PRESET\n  fingerprint PRESET\n  pack TEMPLATE WASM_OR_WAT OUTPUT\n  pin-trace PRESET TRACE_TEMPLATE OUTPUT\n  replay PRESET TRACE [--check]\n  record PRESET PORTS_JSON OUTPUT SECONDS FIRMWARE OS\n  capture-input MIDI_OR_HID DEVICE_ID OUTPUT SECONDS MODEL FIRMWARE OS\nPORTS_JSON maps each logical endpoint to the exact ID printed by devices. Record applies the preset and sends its initialization and feedback reports.".into()),
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

fn capture_input(args: &[String]) -> Result {
    use std::io::BufRead;
    if Path::new(&args[2]).exists() {
        return Err("Capture output already exists".into());
    }
    let seconds = args[3].parse()?;
    let (tx, rx) = std::sync::mpsc::sync_channel(32);
    let marker_loss = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let overflow = marker_loss.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut pending = Vec::with_capacity(200);
        let mut oversized = false;
        loop {
            let Ok(buffer) = stdin.fill_buf() else {
                break;
            };
            if buffer.is_empty() {
                break;
            }
            let consumed = buffer.len();
            for byte in buffer {
                if *byte == b'\n' {
                    if oversized {
                        overflow.store(true, std::sync::atomic::Ordering::Relaxed);
                    } else if let Ok(label) = String::from_utf8(std::mem::take(&mut pending)) {
                        let label = label.trim().to_owned();
                        if !label.is_empty() {
                            match tx.try_send(label) {
                                Ok(()) => {}
                                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                                    overflow.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
                            }
                        }
                    }
                    pending.clear();
                    oversized = false;
                } else if pending.len() < 200 {
                    pending.push(*byte);
                } else {
                    oversized = true;
                }
            }
            stdin.consume(consumed);
        }
    });
    eprintln!("Input observation only. No preset or output reports. Type control labels followed by Enter.");
    let mut capture = mixless_controllers::capture::capture_input(
        &args[0], &args[1], seconds, &args[4], &args[5], &args[6], rx,
    )?;
    capture.truncated |= marker_loss.load(std::sync::atomic::Ordering::Relaxed);
    write_new(Path::new(&args[2]), &capture)?;
    if capture.truncated {
        return Err("Saved truncated input observation; not a reviewed replay fixture".into());
    }
    println!(
        "Observed {} frames. Hardware compatibility has not been verified.",
        capture.frames.len()
    );
    Ok(())
}
