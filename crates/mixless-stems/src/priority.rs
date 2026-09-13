/// Model threads inherit utility QoS on macOS, preserving interactive/audio work.
pub fn background() {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(class: u32, relative: i32) -> i32;
        }
        let _ = pthread_set_qos_class_self_np(0x11, 0); // QOS_CLASS_UTILITY
    }
}
