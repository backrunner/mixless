//! Rust pthreads start at default QoS on macOS. Carry the caller's scheduling
//! class into scoped workers so background ordering stays below playback.
#[derive(Clone, Copy)]
pub(crate) struct Scheduling {
    #[cfg(target_os = "macos")]
    class: u32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn qos_class_self() -> u32;
    fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
}

impl Scheduling {
    pub(crate) fn current() -> Self {
        Self {
            // SAFETY: Darwin's query takes no arguments and has no preconditions.
            #[cfg(target_os = "macos")]
            class: unsafe { qos_class_self() },
        }
    }

    pub(crate) fn enter(self) -> Guard {
        let previous = Self::current();
        self.apply();
        Guard(previous, std::marker::PhantomData)
    }

    pub(crate) fn apply(self) {
        #[cfg(target_os = "macos")]
        {
            // An unspecified pthread uses default QoS. Reused pool workers
            // must not retain an earlier background job's explicit class.
            let class = if self.class == 0 { 0x15 } else { self.class };
            // SAFETY: class came from Darwin; zero is a valid relative priority.
            unsafe {
                pthread_set_qos_class_self_np(class, 0);
            }
        }
    }
}

/// Restore the worker after nested jobs with a different caller QoS.
pub(crate) struct Guard(Scheduling, std::marker::PhantomData<std::rc::Rc<()>>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.apply();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn unspecified_callers_reset_reused_workers_to_default_qos() {
        std::thread::spawn(|| {
            Scheduling { class: 0x11 }.apply();
            {
                let _foreground = Scheduling { class: 0 }.enter();
                assert_eq!(unsafe { qos_class_self() }, 0x15);
            }
            assert_eq!(unsafe { qos_class_self() }, 0x11);
        })
        .join()
        .unwrap();
    }
}
