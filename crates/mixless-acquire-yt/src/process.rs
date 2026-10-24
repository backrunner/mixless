//! Bounded subprocess execution shared by tool checks and downloads.
use crate::err;
use mixless_acquire::AcquireError;
use std::{
    io::{Read, Seek, SeekFrom},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

pub(super) fn run(mut command: Command, deadline: Instant) -> Result<Output, AcquireError> {
    if Instant::now() >= deadline {
        return Err(err("Audio search/download timed out"));
    }
    let mut stdout = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
    let mut stderr = tempfile::tempfile().map_err(|e| err(e.to_string()))?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| err(e.to_string()))?)
        .stderr(stderr.try_clone().map_err(|e| err(e.to_string()))?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| AcquireError::ToolUnavailable(format!("Cannot start yt-dlp: {e}")))?;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                terminate(&mut child);
                return Err(err(error.to_string()));
            }
        }
        if Instant::now() >= deadline
            || stdout.metadata().is_ok_and(|m| m.len() > 8 * 1024 * 1024)
            || stderr.metadata().is_ok_and(|m| m.len() > 8 * 1024 * 1024)
        {
            terminate(&mut child);
            return Err(err(
                "Audio tool timed out or returned excessive output; retry later",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    stdout
        .seek(SeekFrom::Start(0))
        .map_err(|e| err(e.to_string()))?;
    stderr
        .seek(SeekFrom::Start(0))
        .map_err(|e| err(e.to_string()))?;
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout
        .take(8 * 1024 * 1024)
        .read_to_end(&mut output.stdout)
        .map_err(|e| err(e.to_string()))?;
    stderr
        .take(8192)
        .read_to_end(&mut output.stderr)
        .map_err(|e| err(e.to_string()))?;
    Ok(output)
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
