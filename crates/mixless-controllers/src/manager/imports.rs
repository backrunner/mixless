//! Import review and one transactional save for all accepted presets.
use super::*;

pub const MAX_IMPORT_FILES: usize = 128;
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone)]
pub struct ImportEntry {
    pub file_name: String,
    pub result: Result<ImportReport, String>,
}
#[derive(Debug, Clone)]
pub struct ImportBatch {
    pub entries: Vec<ImportEntry>,
}
impl ImportBatch {
    pub fn ready_count(&self) -> usize {
        self.entries.iter().filter(|e| e.result.is_ok()).count()
    }
    pub fn skipped_count(&self) -> usize {
        self.entries.len() - self.ready_count()
    }
}

fn validate_import(p: &ControllerPreset) -> Result<(), MidiError> {
    p.validate()?;
    if p.adapter.is_some() {
        crate::adapter::AdapterInstance::new(p)?;
    }
    Ok(())
}
fn report(preset: ControllerPreset, all: &[ControllerPreset]) -> ImportReport {
    let conflicts = all
        .iter()
        .filter(|p| p.id == preset.id || p.name == preset.name)
        .map(|p| {
            format!(
                "{} already exists; import will create a separate copy",
                p.name
            )
        })
        .collect();
    let requirements = preset
        .endpoints
        .iter()
        .map(|e| match &e.matcher {
            DeviceMatcher::Midi { names } => format!("{}: MIDI {}", e.id, names.join(", ")),
            DeviceMatcher::Hid {
                vendor_id,
                product_id,
                ..
            } => format!("{}: HID {:04X}:{:04X}", e.id, vendor_id, product_id),
        })
        .collect();
    ImportReport {
        preset,
        conflicts,
        requirements,
    }
}
impl ControllerManager {
    pub fn preview_import(&self, path: &Path) -> Result<ImportReport, MidiError> {
        let preset = read_preset(path)?;
        validate_import(&preset)?;
        Ok(report(preset, &self.presets()))
    }

    /// Reads every selected file without modifying the library or opening devices.
    /// Invalid files stay in the review so callers can report each failure.
    pub fn preview_imports(&self, paths: &[PathBuf]) -> Result<ImportBatch, MidiError> {
        if paths.is_empty() || paths.len() > MAX_IMPORT_FILES {
            return Err(error(format!(
                "Select between 1 and {MAX_IMPORT_FILES} preset files"
            )));
        }
        let bytes = paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .try_fold(0u64, |sum, m| sum.checked_add(m.len()));
        if bytes.is_none_or(|bytes| bytes > MAX_IMPORT_BYTES) {
            return Err(error(
                "Selected files exceed 64 MiB; choose a smaller batch",
            ));
        }
        let all = self.presets();
        let mut entries: Vec<_> = paths
            .iter()
            .map(|path| ImportEntry {
                file_name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                result: (|| {
                    let p = read_preset(path)?;
                    validate_import(&p)?;
                    Ok(report(p, &all))
                })()
                .map_err(|e: MidiError| e.to_string()),
            })
            .collect();
        let identities: Vec<_> = entries
            .iter()
            .filter_map(|e| e.result.as_ref().ok())
            .map(|r| (r.preset.id.clone(), r.preset.name.clone()))
            .collect();
        for entry in &mut entries {
            if let Ok(r) = &mut entry.result {
                if identities
                    .iter()
                    .filter(|(id, name)| id == &r.preset.id || name == &r.preset.name)
                    .count()
                    > 1
                {
                    r.conflicts.push("Another selected file has the same preset name or ID; each will be saved as a separate copy".into());
                }
            }
        }
        Ok(ImportBatch { entries })
    }

    pub fn accept_import(&self, report: ImportReport) -> Result<String, MidiError> {
        Ok(self.accept_imports(vec![report])?.remove(0))
    }

    /// Revalidates every report, then commits once. A failure preserves the entire
    /// library and active assignments, even when an earlier report was valid.
    pub fn accept_imports(&self, reports: Vec<ImportReport>) -> Result<Vec<String>, MidiError> {
        if reports.is_empty() || reports.len() > MAX_IMPORT_FILES {
            return Err(error(format!(
                "Import between 1 and {MAX_IMPORT_FILES} presets"
            )));
        }
        let mut presets = Vec::with_capacity(reports.len());
        for report in reports {
            let mut p = report.preset;
            p.id = new_id();
            p.verification = Verification::Protocol;
            validate_import(&p)?;
            presets.push(p);
        }
        let ids = presets.iter().map(|p| p.id.clone()).collect();
        self.store.lock().unwrap().edit(|state| {
            state.presets.extend(presets);
            Ok(())
        })?;
        self.changed();
        Ok(ids)
    }
}
