//! Bounded read-only subprocesses, always reaped and never run through a shell.
use std::{
    io::{Read, Seek},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) fn output(command: &mut Command) -> Result<(ExitStatus, Vec<u8>), String> {
    let mut output = tempfile::tempfile().map_err(|e| e.to_string())?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => e.to_string(),
                    _ => "Process discovery or metadata query timed out".into(),
                });
            }
        }
    };
    output.rewind().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    output
        .take(8 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok((status, bytes))
}
