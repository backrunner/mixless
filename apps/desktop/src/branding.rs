//! Embedded brand assets work in bundles and source-tree development launches.
use gpui::{HighlightStyle, Image, ImageFormat, StyledText};
use std::sync::{Arc, OnceLock};

pub fn wordmark() -> StyledText {
    StyledText::new("MIXLESS.").with_highlights([(
        7..8,
        HighlightStyle {
            color: Some(crate::theme::BRAND_YELLOW.into()),
            ..Default::default()
        },
    )])
}

pub fn icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../resources/Mixless.iconset/icon_256x256.png").to_vec(),
        ))
    })
    .clone()
}

pub fn mark() -> Arc<Image> {
    static MARK: OnceLock<Arc<Image>> = OnceLock::new();
    MARK.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../resources/Mixless.iconset/icon_128x128.png").to_vec(),
        ))
    })
    .clone()
}

pub fn set_dock_icon() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn mixless_set_app_icon(bytes: *const u8, length: usize);
        }
        let bytes = include_bytes!("../resources/Mixless.icns");
        // Called inside Application::run on the main thread. NSData copies the
        // embedded bytes, and AppKit retains the image under Objective-C ARC.
        unsafe { mixless_set_app_icon(bytes.as_ptr(), bytes.len()) };
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mixless_configure_main_window();
    fn mixless_drag_main_window();
}
pub fn configure_main_window() {
    #[cfg(target_os = "macos")]
    unsafe {
        mixless_configure_main_window();
    }
}
pub fn drag_main_window() {
    #[cfg(target_os = "macos")]
    unsafe {
        mixless_drag_main_window();
    }
}
