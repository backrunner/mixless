//! Keep hidapi's process-global macOS IOHIDManager on one process-lifetime thread.
//! Its run loop must outlive short-lived ControllerManagers and their scan workers.
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::{
    ffi::{CStr, CString},
    sync::{mpsc, OnceLock},
    thread,
    time::Duration,
};

type Reply<T> = mpsc::Sender<Result<T, String>>;
pub(crate) type Pending<T> = mpsc::Receiver<Result<T, String>>;
pub(crate) struct OpenedDevice {
    pub device: HidDevice,
    pub numbered: bool,
}
enum Request {
    List(Reply<Vec<DeviceInfo>>),
    Open(CString, Reply<OpenedDevice>),
}
fn sender() -> Result<&'static mpsc::Sender<Request>, String> {
    static SENDER: OnceLock<Result<mpsc::Sender<Request>, String>> = OnceLock::new();
    SENDER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            thread::Builder::new()
                .name("mixless-hid-discovery".into())
                .spawn(move || {
                    let mut api = None;
                    while let Ok(request) = rx.recv() {
                        if api.is_none() {
                            match HidApi::new() {
                                Ok(context) => api = Some(context),
                                Err(e) => {
                                    match request {
                                        Request::List(reply) => {
                                            let _ = reply.send(Err(e.to_string()));
                                        }
                                        Request::Open(_, reply) => {
                                            let _ = reply.send(Err(e.to_string()));
                                        }
                                    }
                                    continue;
                                }
                            }
                        }
                        let api = api.as_mut().unwrap();
                        match request {
                            Request::List(reply) => {
                                let result = api
                                    .refresh_devices()
                                    .map(|()| api.device_list().cloned().collect())
                                    .map_err(|e| e.to_string());
                                let _ = reply.send(result);
                            }
                            Request::Open(path, reply) => {
                                let result = api
                                    .open_path(&path)
                                    .and_then(|device| {
                                        device.set_blocking_mode(false)?;
                                        let mut descriptor = [0u8; 4096];
                                        let numbered = device
                                            .get_report_descriptor(&mut descriptor)
                                            .map(|n| has_report_id(&descriptor[..n]))
                                            .unwrap_or(true);
                                        Ok(OpenedDevice { device, numbered })
                                    })
                                    .map_err(|e| e.to_string());
                                let _ = reply.send(result);
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(tx)
        })
        .as_ref()
        .map_err(Clone::clone)
}
fn response<T>(rx: mpsc::Receiver<Result<T, String>>) -> Result<T, String> {
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|e| format!("HID discovery unavailable: {e}"))?
}
pub(crate) fn devices() -> Result<Vec<DeviceInfo>, String> {
    response(request_devices()?)
}
pub(crate) fn request_devices() -> Result<Pending<Vec<DeviceInfo>>, String> {
    let (tx, rx) = mpsc::channel();
    sender()?
        .send(Request::List(tx))
        .map_err(|e| e.to_string())?;
    Ok(rx)
}
pub(crate) fn request_open(path: &CStr) -> Result<Pending<OpenedDevice>, String> {
    let (tx, rx) = mpsc::channel();
    sender()?
        .send(Request::Open(path.into(), tx))
        .map_err(|e| e.to_string())?;
    Ok(rx)
}

fn has_report_id(descriptor: &[u8]) -> bool {
    let mut i = 0;
    while i < descriptor.len() {
        let b = descriptor[i];
        if b == 0xfe {
            if i + 2 >= descriptor.len() {
                return false;
            }
            i += 3 + descriptor[i + 1] as usize;
            continue;
        }
        let size = match b & 3 {
            3 => 4,
            n => n as usize,
        };
        if b & 0xfc == 0x84 {
            return true;
        }
        i += 1 + size;
    }
    false
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn native_hid_discovery_survives_short_lived_parallel_callers() {
        // Enumerate only: no device is opened, configured or claimed as validated.
        // In the previous implementation the first caller left a dangling CFRunLoop.
        for _ in 0..4 {
            let callers: Vec<_> = (0..8).map(|_| std::thread::spawn(super::devices)).collect();
            for caller in callers {
                caller.join().unwrap().unwrap();
            }
        }
    }
}

/// Shared stable identity for enumeration and explicitly selected input capture.
pub(crate) fn identity(info: &DeviceInfo) -> String {
    let identity = info
        .serial_number()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| info.path().to_string_lossy().into_owned());
    format!(
        "hid:{:04x}:{:04x}:{}:{}:{}:{}",
        info.vendor_id(),
        info.product_id(),
        identity,
        info.interface_number(),
        info.usage_page(),
        info.usage()
    )
}
