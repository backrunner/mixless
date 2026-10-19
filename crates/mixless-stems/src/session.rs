//! Provider policy and recoverable sessions; all work stays on model workers.
use crate::{check, Error, Result};
use ort::{
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session,
    },
    value::{DynValue, Tensor},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Backend {
    Auto,
    Mlx,
    Cpu,
    CoreMl,
}
impl Backend {
    pub fn environment() -> Result<Self> {
        match std::env::var("MIXLESS_STEMS_BACKEND").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("auto") => Ok(Self::Auto),
            Ok("mlx") => Ok(Self::Mlx),
            Ok("cpu") => Ok(Self::Cpu),
            Ok("coreml") => Ok(Self::CoreMl),
            _ => Err(Error::Model(
                "MIXLESS_STEMS_BACKEND must be auto, mlx, coreml, or cpu".into(),
            )),
        }
    }
}

impl Backend {
    fn candidates(self) -> &'static [Self] {
        match self {
            Self::Auto | Self::Mlx => &[Self::Mlx, Self::Cpu],
            Self::CoreMl => &[Self::CoreMl, Self::Cpu],
            Self::Cpu => &[Self::Cpu],
        }
    }
    fn next(self) -> Self {
        Self::Cpu
    }
    fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mlx => "mlx+cpu",
            Self::CoreMl => "coreml+cpu",
            Self::Cpu => "cpu",
        }
    }
}

pub(super) struct SessionState {
    session: Option<Session>,
    path: PathBuf,
    threads: usize,
    backend: Backend,
    cache: Option<PathBuf>,
    profiling: bool,
    name: &'static str,
}

fn builder(threads: usize, name: &str) -> Result<(SessionBuilder, bool)> {
    let mut builder = Session::builder()?
        .with_intra_threads(threads)?
        .with_inter_threads(1)?
        .with_parallel_execution(false)?
        .with_intra_op_spinning(false)?
        .with_inter_op_spinning(false)?
        .with_optimization_level(GraphOptimizationLevel::Level3)?;
    let profile = std::env::var_os("MIXLESS_ORT_PROFILE_DIR");
    if let Some(root) = &profile {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root)?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        builder =
            builder.with_profiling(root.join(format!("{name}-{}-{id}", std::process::id())))?;
    }
    Ok((builder, profile.is_some()))
}

impl SessionState {
    pub fn load(
        path: &Path,
        threads: usize,
        name: &'static str,
        backend: Backend,
        cache: Option<&Path>,
        active: &impl Fn() -> bool,
    ) -> Result<Self> {
        check(active)?;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        crate::runtime::initialize();
        let at = Instant::now();
        for candidate in backend.candidates() {
            check(active)?;
            let loaded = Self::create(path, threads, name, *candidate, cache, active);
            match loaded {
                Ok((session, profiling)) => {
                    tracing::info!(
                        model = name,
                        provider = candidate.label(),
                        seconds = at.elapsed().as_secs_f64(),
                        "Loaded inference session"
                    );
                    return Ok(Self {
                        session: Some(session),
                        path: path.into(),
                        threads,
                        backend: *candidate,
                        cache: cache.map(Path::to_owned),
                        profiling,
                        name,
                    });
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) if *candidate != Backend::Cpu => {
                    tracing::warn!(model = name, provider = candidate.label(), %error,
                        "Inference backend unavailable; trying the next backend");
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("Every backend chain ends at CPU")
    }

    fn create(
        path: &Path,
        threads: usize,
        name: &str,
        backend: Backend,
        cache: Option<&Path>,
        active: &impl Fn() -> bool,
    ) -> Result<(Session, bool)> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        match backend {
            Backend::Mlx => {
                crate::runtime::register_mlx()?;
                let env = ort::environment::Environment::current()?;
                let devices: Vec<_> = env
                    .devices()
                    .filter(|d| {
                        d.ep()
                            .is_ok_and(|ep| ep == mixless_protocol::inference_runtime::MLX_PROVIDER)
                    })
                    .collect();
                if devices.is_empty() {
                    return Err(Error::Model("No MLX device available".into()));
                }
                let (builder, profiling) = builder(threads, name)?;
                let session = builder
                    .with_devices(devices, None)?
                    .commit_from_file(path)?;
                return Ok((session, profiling));
            }
            Backend::CoreMl => {
                // This attention graph's CoreML compilation exceeded 10 GiB in
                // native validation. Keep the default MLX -> CPU chain bounded;
                // explicit CoreML requests for this model also fall back to CPU.
                if name == "separator-bs-roformer" {
                    return Err(Error::Model(
                        "CoreML is unavailable for the pinned BS-RoFormer graph".into(),
                    ));
                }
                if !name.starts_with("separator") {
                    // This fixed Basic Pitch graph fails MLProgram's Conv pad
                    // conversion. Treat it as unsupported and continue to CPU.
                    return Err(Error::Model(
                        "CoreML does not support the fixed Basic Pitch graph".into(),
                    ));
                }
                return Self::coreml(path, threads, cache, active);
            }
            _ => {}
        }
        let _ = (cache, active);
        if backend != Backend::Cpu {
            return Err(Error::Model("Backend unsupported on this platform".into()));
        }
        let (mut builder, profiling) = builder(threads, name)?;
        Ok((builder.commit_from_file(path)?, profiling))
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    fn coreml(
        path: &Path,
        threads: usize,
        cache: Option<&Path>,
        active: &impl Fn() -> bool,
    ) -> Result<(Session, bool)> {
        use ort::ep::{self, ExecutionProvider};
        if !ep::CoreML::default().is_available()? {
            return Err(Error::Model(
                "Runtime has no CoreML execution provider".into(),
            ));
        }
        let mut ep = ep::CoreML::default()
            .with_model_format(ep::coreml::ModelFormat::MLProgram)
            .with_compute_units(ep::coreml::ComputeUnits::CPUAndGPU);
        let cache = cache.map(|root| {
            root.join(crate::runtime::cache_version())
                .join(crate::coreml::CACHE_VERSION)
                .join(crate::models::SEPARATOR_HASH)
        });
        // Both pooled sessions/processes may cold-load together. Never compile
        // into the same cache concurrently; cancellation also applies to waiting.
        let _lock = if let Some(cache) = &cache {
            std::fs::create_dir_all(cache)?;
            let lock = crate::lock::acquire(&cache.join(".compile.lock"), active)?;
            ep = ep.with_model_cache_dir(cache.display());
            Some(lock)
        } else {
            None
        };
        check(active)?;
        let model = crate::coreml::separator(path)?;
        let (builder, profiling) = builder(threads, "separator-coreml")?;
        let session = builder
            .with_execution_providers([ep.build().error_on_failure()])?
            .commit_from_memory(&model)?;
        Ok((session, profiling))
    }

    pub fn backend(&self) -> &'static str {
        self.backend.label()
    }

    // Retry the same owned input in descending priority; never promote a failed
    // session again. Cancellation is independent of backend health.
    #[cfg(test)]
    pub fn run(
        &mut self,
        input_name: &str,
        input: &Tensor<f32>,
        outputs: &[&str],
        active: &impl Fn() -> bool,
    ) -> Result<Vec<DynValue>> {
        self.run_named_inputs(&[(input_name, input)], outputs, active)
    }

    pub fn run_inputs(
        &mut self,
        inputs: &[(String, Tensor<f32>)],
        outputs: &[&str],
        active: &impl Fn() -> bool,
    ) -> Result<Vec<DynValue>> {
        let borrowed: Vec<_> = inputs
            .iter()
            .map(|(name, input)| (name.as_str(), input))
            .collect();
        self.run_named_inputs(&borrowed, outputs, active)
    }

    fn run_named_inputs(
        &mut self,
        inputs: &[(&str, &Tensor<f32>)],
        outputs: &[&str],
        active: &impl Fn() -> bool,
    ) -> Result<Vec<DynValue>> {
        loop {
            check(active)?;
            match self.run_once(inputs, outputs) {
                Ok(values) => {
                    check(active)?;
                    return Ok(values);
                }
                Err(error) if self.backend != Backend::Cpu => {
                    check(active)?;
                    tracing::warn!(model = self.name, provider = self.backend.label(), %error,
                        "Inference failed; retrying the same input on the next backend");
                    let _ = self.finish_profiling();
                    self.session.take();
                    *self = Self::load(
                        &self.path,
                        self.threads,
                        self.name,
                        self.backend.next(),
                        self.cache.as_deref(),
                        active,
                    )?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn run_once(
        &mut self,
        inputs: &[(&str, &Tensor<f32>)],
        names: &[&str],
    ) -> Result<Vec<DynValue>> {
        let mut outputs = self
            .session
            .as_mut()
            .ok_or_else(|| Error::Model("Inference session unavailable".into()))?
            .run(
                inputs
                    .iter()
                    .map(|&(name, tensor)| (name, tensor))
                    .collect::<Vec<_>>(),
            )?;
        names
            .iter()
            .map(|name| {
                let value = outputs
                    .remove(*name)
                    .ok_or_else(|| Error::Model(format!("Missing model output: {name}")))?;
                let (shape, _) = value.try_extract_tensor::<f32>()?;
                let expected: &[i64] = if self.name.starts_with("separator") {
                    &[1, 4, 2, 1025, 801]
                } else {
                    &[1, 172, 88]
                };
                if &shape[..] != expected {
                    return Err(Error::Model(format!(
                        "Unexpected model output shape for {name}: {shape:?}"
                    )));
                }
                if value
                    .try_extract_tensor::<f32>()?
                    .1
                    .iter()
                    .any(|v| !v.is_finite())
                {
                    return Err(Error::Model(format!("Non-finite model output: {name}")));
                }
                Ok(value)
            })
            .collect()
    }

    pub fn finish_profiling(&mut self) -> Result<Option<String>> {
        if !self.profiling {
            return Ok(None);
        }
        self.profiling = false;
        let Some(session) = self.session.as_mut() else {
            return Ok(None);
        };
        let path = session.end_profiling()?;
        tracing::info!(model = self.name, %path, "Saved ONNX execution profile");
        Ok(Some(path))
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        let _ = self.finish_profiling();
    }
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    fn inputs() -> Vec<(String, Tensor<f32>)> {
        let mut pcm = vec![0.; crate::spectral::FRAMES * 2];
        for i in 0..crate::spectral::FRAMES {
            pcm[i * 2] = (i as f32 * 0.071).sin() * 0.1;
            pcm[i * 2 + 1] = (i as f32 * 0.043).cos() * 0.08;
        }
        let (real, imag) = crate::spectral::analyze(&pcm, &|| true).unwrap();
        [("spec_real", real), ("spec_imag", imag)]
            .into_iter()
            .map(|(name, data)| {
                (
                    name.into(),
                    Tensor::from_array((
                        [1usize, 2, crate::spectral::BINS, crate::spectral::STEPS],
                        data,
                    ))
                    .unwrap(),
                )
            })
            .collect()
    }
    #[test]
    #[ignore = "Requires BS-RoFormer weights in MIXLESS_TEST_BUNDLED_MODELS; native provider recovery and parity"]
    fn native_bs_roformer_provider_recovery_preserves_both_spectral_inputs() {
        crate::runtime::initialize();
        let models =
            PathBuf::from(std::env::var_os("MIXLESS_TEST_BUNDLED_MODELS").expect("models"));
        let paths =
            crate::models::ensure(&models, Some(&models), false, &mut |_| {}, &|| true).unwrap();
        let input = inputs();
        let names = &["out_spec_real", "out_spec_imag"];
        let mut reference = SessionState::load(
            &paths.separators[0],
            4,
            "separator-bs-roformer",
            Backend::Cpu,
            None,
            &|| true,
        )
        .unwrap();
        let cpu = reference.run_inputs(&input, names, &|| true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = SessionState::load(
            &paths.separators[0],
            4,
            "separator-bs-roformer",
            Backend::Auto,
            Some(dir.path()),
            &|| true,
        )
        .unwrap();
        let out = session.run_inputs(&input, names, &|| true).unwrap();
        let mut max = 0f32;
        for (expected, actual) in cpu.iter().zip(&out) {
            for (a, b) in expected
                .try_extract_tensor::<f32>()
                .unwrap()
                .1
                .iter()
                .zip(actual.try_extract_tensor::<f32>().unwrap().1)
            {
                max = max.max((a - b).abs());
            }
        }
        eprintln!(
            "CPU / {} spectral max absolute difference: {max}",
            session.backend()
        );
        assert!(max < 1e-3);
        assert!(matches!(
            session.run_inputs(&input, names, &|| false),
            Err(Error::Cancelled)
        ));
        session.session.take();
        if session.backend != Backend::Cpu {
            let recovered = session.run_inputs(&input, names, &|| true).unwrap();
            assert_eq!(recovered.len(), 2);
        }
        // Explicit CoreML requests must fall back before touching a cache path
        // and still accept both graph inputs on CPU.
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        let mut fallback = SessionState::load(
            &paths.separators[0],
            4,
            "separator-bs-roformer",
            Backend::CoreMl,
            Some(&blocked),
            &|| true,
        )
        .unwrap();
        assert_eq!(fallback.backend(), "cpu");
        assert!(fallback.run_inputs(&input, names, &|| true).is_ok());
        // Basic Pitch retains its one-input path and cancellation behavior.
        let mut notes =
            SessionState::load(&paths.notes, 4, "notes", Backend::Cpu, None, &|| true).unwrap();
        let audio = Tensor::from_array(([1usize, 43844, 1], vec![0.; 43844])).unwrap();
        assert!(notes
            .run(
                "serving_default_input_2:0",
                &audio,
                &["StatefulPartitionedCall:1", "StatefulPartitionedCall:2"],
                &|| true
            )
            .is_ok());
    }
}
