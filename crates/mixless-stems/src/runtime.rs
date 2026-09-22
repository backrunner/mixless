//! Select one ORT API for the process before creating any tensors or sessions.
//! The static runtime remains usable if the optional newer dylib cannot load.
use crate::{Error, Result};
use mixless_protocol::inference_runtime::{MLX_LIBRARY, MLX_PROVIDER, ORT_LIBRARY, VERSION};
use std::{path::PathBuf, sync::OnceLock};

struct Runtime {
    // A selected API contains function pointers into this library. Never unload
    // it while any ORT value, environment, provider or session could still exist.
    _library: Option<libloading::Library>,
    directory: Option<PathBuf>,
}

fn directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MIXLESS_INFERENCE_RUNTIME_DIR") {
        return Some(path.into());
    }
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    if parent.file_name().is_some_and(|p| p == "MacOS") {
        // Packaged apps must never accidentally depend on a checkout's files.
        return Some(parent.parent()?.join("Frameworks"));
    }
    // Cargo binaries (including examples/tests) discover assets alongside the
    // target directory, including --target builds and custom CARGO_TARGET_DIR.
    exe.ancestors()
        .skip(1)
        .map(|p| p.join("inference-runtime").join(VERSION))
        .find(|p| p.join(ORT_LIBRARY).is_file())
}

fn load() -> std::result::Result<Runtime, Box<dyn std::error::Error>> {
    let mut version = [0u8; 64];
    let mut size = version.len();
    let status = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            version.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    let major = std::str::from_utf8(&version)
        .ok()
        .and_then(|s| s.split('.').next()?.parse::<u32>().ok());
    if status != 0 || major.is_none_or(|v| v < 14) {
        return Err("Optional MLX runtime requires macOS 14 or newer".into());
    }
    let directory = directory().ok_or("Optional MLX runtime is not installed")?;
    // dlopen checks the dylib's architecture, minimum OS and dependencies. This
    // happens lazily during analysis, not during app launch on older systems.
    let library = unsafe { libloading::Library::new(directory.join(ORT_LIBRARY))? };
    let base = unsafe {
        let get: libloading::Symbol<unsafe extern "C" fn() -> *const ort::sys::OrtApiBase> =
            library.get(b"OrtGetApiBase\0")?;
        get().as_ref().ok_or("Missing ORT API base")?
    };
    let api = unsafe {
        (base.GetApi)(29)
            .as_ref()
            .ok_or("ORT API 29 is unavailable")?
    };
    // ORT guarantees backward-compatible API prefixes. The Rust binding clones
    // only its declared API table (27); plugin registration receives API 29 from
    // the selected runtime. No values may be migrated between runtimes.
    if !ort::set_api(api.clone()) {
        return Err("ORT was already initialized; keeping the existing runtime".into());
    }
    tracing::info!(runtime = VERSION, "Selected bundled inference runtime");
    Ok(Runtime {
        _library: Some(library),
        directory: Some(directory),
    })
}

fn current() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| match load() {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::info!(%error, "Using built-in ONNX Runtime (CoreML/CPU)");
            // Lock the static API choice before any other worker initializes.
            let _ = ort::api();
            Runtime {
                _library: None,
                directory: None,
            }
        }
    })
}

pub(super) fn initialize() {
    let _ = current();
}

pub(super) fn cache_version() -> &'static str {
    if current().directory.is_some() {
        VERSION
    } else {
        "ort-1.28"
    }
}

pub(super) fn register_mlx() -> Result<()> {
    static REGISTERED: OnceLock<std::result::Result<libloading::Library, String>> = OnceLock::new();
    match REGISTERED.get_or_init(|| {
        let directory = current()
            .directory
            .as_ref()
            .ok_or("MLX runtime unavailable")?;
        let adjacent = directory.join("mlx.metallib");
        let metal = if adjacent.is_file() {
            adjacent
        } else {
            directory
                .parent()
                .ok_or("Missing app resources")?
                .join("Resources/inference-runtime/mlx.metallib")
        };
        if !metal.is_file() {
            return Err("MLX Metal kernels are missing".into());
        }
        // Metal kernels are data, so signed apps put them in Resources.
        // Set the supported mlx-c path override before creating a device;
        // retain the library along with the registered plugin for its lifetime.
        let bridge = unsafe { libloading::Library::new(directory.join("libmlxc.dylib")) }
            .map_err(|e| e.to_string())?;
        let path = std::ffi::CString::new(metal.as_os_str().as_encoded_bytes())
            .map_err(|e| e.to_string())?;
        unsafe {
            let set_path: libloading::Symbol<unsafe extern "C" fn(*const std::ffi::c_char) -> i32> =
                bridge
                    .get(b"mlx_metal_set_metallib_path\0")
                    .map_err(|e| e.to_string())?;
            if set_path(path.as_ptr()) != 0 {
                return Err("Could not configure MLX Metal kernels".into());
            }
        }
        let env = ort::environment::Environment::current().map_err(|e| e.to_string())?;
        env.register_ep_library(MLX_PROVIDER, directory.join(MLX_LIBRARY))
            .map_err(|e| e.to_string())?;
        Ok(bridge)
    }) {
        Ok(_) => Ok(()),
        Err(error) => Err(Error::Model(error.clone())),
    }
}
