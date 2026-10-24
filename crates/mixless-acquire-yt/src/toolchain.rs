use std::path::{Path, PathBuf};

pub(super) fn locate(name: &str) -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join(name)));
    beside
        .filter(|p| p.is_file())
        .or_else(|| which::which(name).ok())
        .or_else(|| {
            ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
                .iter()
                .map(|dir| Path::new(dir).join(name))
                .find(|p| p.is_file())
        })
}
