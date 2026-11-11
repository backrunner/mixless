/// Explicit action only: Return and Escape both keep the newer data.
pub(super) fn confirm(description: &str, action: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn mixless_confirm_replacement(
                description: *const std::ffi::c_char,
                action: *const std::ffi::c_char,
            ) -> i32;
        }
        let (Ok(description), Ok(action)) = (CString::new(description), CString::new(action))
        else {
            return false;
        };
        // Both strings live through the synchronous, main-thread AppKit call.
        unsafe { mixless_confirm_replacement(description.as_ptr(), action.as_ptr()) == 1 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        rfd::MessageDialog::new()
            .set_title("mixless")
            .set_description(description)
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
                "Quit".into(),
                action.into(),
                "Cancel".into(),
            ))
            .show()
            == rfd::MessageDialogResult::Custom(action.into())
    }
}
