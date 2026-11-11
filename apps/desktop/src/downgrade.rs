//! Startup compatibility gate. No engine, MIDI, library migration or model
//! worker is started until the user has resolved an incompatible downgrade.
mod assets;
#[cfg(test)]
mod tests;
mod versions;
mod warning;

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use assets::{Assets, Moves};
use fs2::FileExt;
use versions::{Stamp, library_versions};

const STAMP: &str = "data-version.json";

#[cfg(target_os = "macos")]
pub(crate) fn installed_app_is_newer(installed: &str) -> bool {
    versions::newer_app(installed, env!("CARGO_PKG_VERSION"))
}

#[cfg(target_os = "macos")]
pub(crate) fn confirm_install(description: &str) -> bool {
    warning::confirm(description, "Install older version")
}

pub(crate) struct Session {
    _lock: fs::File,
    pub data_dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
struct Report {
    previous: Option<Stamp>,
    unknown_stamp: bool,
    library: mixless_library::downgrade::Inspection,
    assets: Assets,
    reset_preferences: bool,
}

impl Report {
    fn needs_consent(&self) -> bool {
        self.unknown_stamp
            || self
                .previous
                .as_ref()
                .is_some_and(|p| p.newer_than(&Stamp::current()))
            || self.library.needs_consent(library_versions())
            || !self.assets.incompatible_models.is_empty()
            || !self.assets.incompatible_stems.is_empty()
            || self.reset_preferences
    }

    fn description(&self) -> String {
        let heading = if let Some(previous) = self
            .previous
            .as_ref()
            .filter(|p| p.newer_than(&Stamp::current()))
        {
            format!(
                "This data was used by a newer build of mixless ({}). You are opening {}.",
                previous.app,
                env!("CARGO_PKG_VERSION")
            )
        } else {
            "This version of mixless found newer or incompatible library, analysis or model data."
                .into()
        };
        let preferences = if self.reset_preferences {
            " Incompatible preferences will be reset."
        } else {
            ""
        };
        format!(
            "{heading}\n\nUse the latest version of mixless to keep this data.\n\nContinuing backs up the current data, then replaces incompatible analyses and model caches with this version's results. Audio files, playlists and manual cues are kept. Unsupported newer features may be removed.{preferences}\n\nThe backup is saved in the app data folder under downgrade-backups."
        )
    }
}

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("MIXLESS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("app.mixless.desktop")
        })
}

fn model_dir(data: &Path) -> PathBuf {
    std::env::var_os("MIXLESS_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data.join("models"))
}

pub(crate) fn prepare_or_exit() -> Session {
    let data = data_dir();
    match prepare(&data, &model_dir(&data), |report| {
        warning::confirm(&report.description(), "Continue and replace data")
    }) {
        Ok(Some(session)) => session,
        Ok(None) => std::process::exit(0),
        Err(error) => {
            tracing::error!(%error, "compatibility check failed");
            rfd::MessageDialog::new().set_title("mixless")
                .set_description(format!("Could not prepare this library. Use the latest version of mixless, or close other copies and try again.\n\n{error}"))
                .set_level(rfd::MessageLevel::Error).show();
            std::process::exit(1);
        }
    }
}

fn inspect(data: &Path, models: &Path) -> Result<Report, String> {
    let (previous, unknown_stamp) = match fs::read(data.join(STAMP)) {
        Ok(bytes) => match serde_json::from_slice::<Stamp>(&bytes) {
            Ok(stamp) => (Some(stamp), false),
            Err(_) => (None, true),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (None, false),
        Err(e) => return Err(e.to_string()),
    };
    let reset_preferences = match fs::read(data.join("preferences.json")) {
        Ok(bytes) => serde_json::from_slice::<crate::settings::Settings>(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|s| s.validate())
            .is_err(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    Ok(Report {
        library: mixless_library::downgrade::inspect(&data.join("library.db"), library_versions())
            .map_err(|e| e.to_string())?,
        assets: assets::inspect(data, models, previous.as_ref()).map_err(|e| e.to_string())?,
        previous,
        unknown_stamp,
        reset_preferences,
    })
}

fn lock(path: &Path) -> Result<fs::File, String> {
    fs::create_dir_all(path.parent().expect("lock parent")).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive().map_err(|_| {
        "Another copy of mixless is using this data. Close it before continuing.".to_string()
    })?;
    Ok(file)
}

fn prepare(
    data: &Path,
    models: &Path,
    confirm: impl FnOnce(&Report) -> bool,
) -> Result<Option<Session>, String> {
    let lease = lock(&data.join(".compatibility.lock"))?;
    let report = inspect(data, models)?;
    if report.needs_consent() {
        if !confirm(&report) {
            return Ok(None);
        }
        // Existing stem workers (including older builds) use these locks.
        // Never relocate a cache or weight file out from under inference.
        let _analysis = lock(&data.join("stem-cache/.analysis.lock"))?;
        let _models = lock(&models.join(".models.lock"))?;
        if inspect(data, models)? != report {
            return Err("The data changed while the warning was open. Reopen mixless to review the new changes.".into());
        }
        convert(data, &report)?;
    }
    let mut stamp = Stamp::current();
    stamp.caches = report
        .assets
        .caches
        .into_iter()
        .filter(|cache| {
            !report
                .assets
                .incompatible_stems
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == cache.name.as_str()))
        })
        .collect();
    stamp.models = report
        .assets
        .models
        .into_iter()
        .filter(|file| {
            mixless_protocol::STEM_MODELS
                .iter()
                .any(|m| m.name == file.name && m.sha256 == file.sha256)
        })
        .collect();
    atomic_json(&data.join(STAMP), &stamp)?;
    Ok(Some(Session {
        _lock: lease,
        data_dir: data.into(),
    }))
}

fn atomic_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    use io::Write;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().expect("data parent"))
        .map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(|e| e.to_string())?;
    temp.flush()
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn convert(data: &Path, report: &Report) -> Result<(), String> {
    let root = data.join("downgrade-backups");
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let backup = tempfile::Builder::new()
        .prefix("before-downgrade-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?
        .keep();
    // Persist the explanation first. Backups are never automatically evicted.
    atomic_json(
        &backup.join("replacement.json"),
        &serde_json::json!({
            "replacement": Stamp::current(), "previous": report.previous,
            "models": report.assets.incompatible_models,
            "stems": report.assets.incompatible_stems,
            "note": "Restore only with mixless closed. library.db includes committed WAL pages."
        }),
    )?;
    let db = data.join("library.db");
    let snapshot = mixless_library::downgrade::backup(&db, &backup.join("library.db"))
        .map_err(|e| e.to_string())?;
    for name in [STAMP, "preferences.json", "midi.json"] {
        if data.join(name).exists() {
            fs::copy(data.join(name), backup.join(name)).map_err(|e| e.to_string())?;
        }
    }
    let mut moves = Moves::default();
    for path in &report.assets.incompatible_models {
        moves
            .move_to(
                path,
                &backup
                    .join("models")
                    .join(path.file_name().expect("model name")),
            )
            .map_err(|e| e.to_string())?;
    }
    for path in &report.assets.incompatible_stems {
        moves
            .move_to(
                path,
                &backup
                    .join("stem-cache")
                    .join(path.file_name().expect("cache name")),
            )
            .map_err(|e| e.to_string())?;
    }
    let model_change = !report.assets.incompatible_models.is_empty()
        || report
            .previous
            .as_ref()
            .is_some_and(|p| p.stems > mixless_stems::VERSION);
    if model_change
        || report
            .previous
            .as_ref()
            .is_some_and(|p| p.runtime != mixless_protocol::inference_runtime::VERSION)
    {
        moves
            .move_to(
                &data.join("stem-cache/.coreml"),
                &backup.join("stem-cache/.coreml"),
            )
            .map_err(|e| e.to_string())?;
    }
    if report.reset_preferences {
        // Preserve independently compatible fields instead of resetting all
        // devices and user choices because one newer enum is unknown.
        let original: serde_json::Value = serde_json::from_slice(
            &fs::read(data.join("preferences.json")).map_err(|e| e.to_string())?,
        )
        .unwrap_or_default();
        let mut compatible = serde_json::to_value(crate::settings::Settings::default())
            .map_err(|e| e.to_string())?;
        if let (Some(old), Some(current)) = (original.as_object(), compatible.as_object_mut()) {
            for (key, value) in old {
                if !current.contains_key(key) {
                    continue;
                }
                let previous = current.insert(key.clone(), value.clone());
                let candidate = serde_json::Value::Object(current.clone());
                if serde_json::from_value::<crate::settings::Settings>(candidate)
                    .map_err(|e| e.to_string())
                    .and_then(|s| s.validate())
                    .is_err()
                {
                    current.insert(key.clone(), previous.expect("known preference"));
                }
            }
        }
        // Move the original for rollback, then atomically publish the projection.
        moves
            .move_to(
                &data.join("preferences.json"),
                &backup.join("preferences-original.json"),
            )
            .map_err(|e| e.to_string())?;
        atomic_json(&data.join("preferences.json"), &compatible)?;
    }
    if let Some(snapshot) = snapshot {
        mixless_library::downgrade::replace(&db, &snapshot, library_versions(), model_change)
            .map_err(|e| e.to_string())?;
    }
    moves.commit();
    tracing::warn!(backup=%backup.display(), "replaced incompatible data after downgrade confirmation");
    Ok(())
}
