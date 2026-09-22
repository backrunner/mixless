//! Sessions are created, run, recovered and dropped on one persistent thread.
//! Callers may move between queue threads without violating MLX thread affinity.
use crate::{
    check,
    session::{Backend, SessionState},
    Error, Result,
};
use ort::value::{DynValue, Tensor};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

enum Request {
    Run {
        name: String,
        input: Tensor<f32>,
        outputs: Vec<String>,
        cancelled: Arc<AtomicBool>,
        reply: mpsc::SyncSender<(Result<Vec<DynValue>>, &'static str)>,
    },
    Profile(mpsc::SyncSender<Result<Option<String>>>),
}

pub(super) struct ModelSession {
    sender: Option<mpsc::SyncSender<Request>>,
    thread: Option<JoinHandle<()>>,
    backend: &'static str,
}

// Propagate cancellation while a native call is in flight. Drain its reply before
// returning so a subsequent request cannot accidentally clear its cancellation.
fn receive<T>(
    rx: &mpsc::Receiver<T>,
    cancelled: &AtomicBool,
    active: &impl Fn() -> bool,
) -> Result<T> {
    loop {
        if !active() {
            cancelled.store(true, Ordering::Release);
        }
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => {
                return Ok(result);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err(Error::Model("Inference worker stopped".into())),
        }
    }
}

impl ModelSession {
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
        let path = path.to_owned();
        let cache = cache.map(Path::to_owned);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let (ready, ready_rx) = mpsc::sync_channel(1);
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name(format!("stems-{name}"))
            .spawn(move || {
                crate::priority::background();
                let mut state = match SessionState::load(
                    &path,
                    threads,
                    name,
                    backend,
                    cache.as_deref(),
                    &|| !worker_cancelled.load(Ordering::Acquire),
                ) {
                    Ok(state) => state,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(state.backend())).is_err() {
                    return;
                }
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Run {
                            name,
                            input,
                            outputs,
                            cancelled,
                            reply,
                        } => {
                            let names: Vec<_> = outputs.iter().map(String::as_str).collect();
                            let result = state.run(&name, &input, &names, &|| {
                                !cancelled.load(Ordering::Acquire)
                            });
                            let _ = reply.send((result, state.backend()));
                        }
                        Request::Profile(reply) => {
                            let _ = reply.send(state.finish_profiling());
                        }
                    }
                }
            })?;
        let mut session = Self {
            sender: Some(sender),
            thread: Some(thread),
            backend: "cpu",
        };
        session.backend = receive(&ready_rx, &cancelled, active)??;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        check(active)?;
        Ok(session)
    }

    pub fn backend(&self) -> &'static str {
        self.backend
    }

    pub fn run(
        &mut self,
        name: &str,
        input: Tensor<f32>,
        outputs: &[&str],
        active: &impl Fn() -> bool,
    ) -> Result<Vec<DynValue>> {
        check(active)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(Request::Run {
            name: name.into(),
            input,
            outputs: outputs.iter().map(|s| (*s).into()).collect(),
            cancelled: cancelled.clone(),
            reply,
        })?;
        // Update diagnostics even when cancellation happened during recovery.
        let (result, backend) = receive(&receiver, &cancelled, active)?;
        self.backend = backend;
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        check(active)?;
        result
    }

    fn send(&self, request: Request) -> Result<()> {
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Model("Inference worker closed".into()))?
            .send(request)
            .map_err(|_| Error::Model("Inference worker stopped".into()))
    }

    pub fn finish_profiling(&mut self) -> Result<Option<String>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send(Request::Profile(reply))?;
        receiver
            .recv()
            .map_err(|_| Error::Model("Inference worker stopped".into()))?
    }
}

impl Drop for ModelSession {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires native MLX runtime and MIXLESS_TEST_BUNDLED_MODELS"]
    fn native_mlx_session_survives_changing_callers_and_cancellation() {
        let models = std::path::PathBuf::from(
            std::env::var_os("MIXLESS_TEST_BUNDLED_MODELS").expect("models"),
        );
        let paths =
            crate::models::ensure(&models, Some(&models), false, &mut |_| {}, &|| true).unwrap();
        let mut session =
            ModelSession::load(&paths.notes, 4, "notes", Backend::Auto, None, &|| true).unwrap();
        let run = |session: &mut ModelSession| {
            let input = Tensor::from_array(([1usize, 43844, 1], vec![0f32; 43844])).unwrap();
            let values = session
                .run(
                    "serving_default_input_2:0",
                    input,
                    &["StatefulPartitionedCall:1", "StatefulPartitionedCall:2"],
                    &|| true,
                )
                .unwrap();
            assert_eq!(session.backend(), "mlx+cpu");
            assert_eq!(
                values[0].try_extract_tensor::<f32>().unwrap().0.as_ref(),
                &[1, 172, 88]
            );
        };
        run(&mut session);
        session = thread::spawn(move || {
            run(&mut session);
            session
        })
        .join()
        .unwrap();
        run(&mut session);
        let calls = std::cell::Cell::new(0);
        let input = Tensor::from_array(([1usize, 43844, 1], vec![0f32; 43844])).unwrap();
        assert!(matches!(
            session.run(
                "serving_default_input_2:0",
                input,
                &["StatefulPartitionedCall:1"],
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() == 1
                }
            ),
            Err(Error::Cancelled)
        ));
        run(&mut session);
        thread::spawn(move || drop(session)).join().unwrap();
    }
}
