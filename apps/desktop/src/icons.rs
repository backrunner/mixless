//! Embedded monochrome SVGs, rasterized by GPUI at the display scale.
use std::borrow::Cow;

pub struct Assets;

const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/close.svg",
        include_bytes!("../resources/icons/close.svg"),
    ),
    (
        "icons/repeat.svg",
        include_bytes!("../resources/icons/repeat.svg"),
    ),
    (
        "icons/shuffle.svg",
        include_bytes!("../resources/icons/shuffle.svg"),
    ),
    (
        "icons/pause.svg",
        include_bytes!("../resources/icons/pause.svg"),
    ),
    (
        "icons/play.svg",
        include_bytes!("../resources/icons/play.svg"),
    ),
    (
        "icons/skip.svg",
        include_bytes!("../resources/icons/skip.svg"),
    ),
];

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| (*name).into())
            .collect())
    }
}
