//! Keep GPU initialization/draining and report I/O outside planner workers.
pub fn run<T>(work: impl FnOnce() -> T) -> Result<T, Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os("MIXLESS_GPU_AUDIT") else {
        return Ok(work());
    };
    #[cfg(not(feature = "gpu-audit"))]
    {
        let _ = path;
        Err("MIXLESS_GPU_AUDIT requires --features gpu-audit".into())
    }
    #[cfg(feature = "gpu-audit")]
    {
        let mode = match std::env::var("MIXLESS_GPU_AUDIT_MODE").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("complete") => {
                mixless_mixplan::gpu_audit::Mode::Complete
            }
            Ok("best-effort") => mixless_mixplan::gpu_audit::Mode::BestEffort,
            _ => return Err("MIXLESS_GPU_AUDIT_MODE must be complete or best-effort".into()),
        };
        let (result, report) = mixless_mixplan::gpu_audit::compare(mode, work);
        std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
        Ok(result)
    }
}
