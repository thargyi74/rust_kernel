// SPDX-License-Identifier: GPL-2.0-only
//! Run a command (normally QEMU) with a hard timeout and capture its output.
//!
//! A kernel that faults and spins forever would otherwise hang the test run,
//! so every run has a deadline. On timeout the child is killed and whatever it
//! printed so far is returned, because the last lines usually say what broke.

use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How the child process ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// The process exited by itself; `None` means it was ended by a signal.
    Exited(Option<i32>),
    /// The deadline passed and we killed it.
    TimedOut,
}

/// Everything the child printed (stdout and stderr together) and how it ended.
#[derive(Debug)]
pub struct RunOutcome {
    pub output: String,
    pub exit: Exit,
}

const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Runs `cmd` with stdin closed, capturing stdout and stderr, and kills it if
/// it is still running after `timeout`.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> io::Result<RunOutcome> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let output = Arc::new(Mutex::new(String::new()));
    let readers = [
        spawn_reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), &output),
        spawn_reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>), &output),
    ];

    let deadline = Instant::now() + timeout;
    let exit = loop {
        if let Some(status) = child.try_wait()? {
            break Exit::Exited(status.code());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            break Exit::TimedOut;
        }
        thread::sleep(POLL_INTERVAL);
    };

    for reader in readers.into_iter().flatten() {
        let _ = reader.join();
    }
    let output = std::mem::take(&mut *output.lock().expect("reader thread panicked"));
    Ok(RunOutcome { output, exit })
}

/// Copies a pipe into the shared buffer chunk by chunk until EOF.
fn spawn_reader(
    pipe: Option<Box<dyn Read + Send>>,
    sink: &Arc<Mutex<String>>,
) -> Option<thread::JoinHandle<()>> {
    let mut pipe = pipe?;
    let sink = Arc::clone(sink);
    Some(thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            sink.lock()
                .expect("output buffer poisoned")
                .push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn returns_output_and_exit_code() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo hi; echo oops >&2; exit 3"]);
        let out = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert!(out.output.contains("hi\n") && out.output.contains("oops\n"));
        assert_eq!(out.exit, Exit::Exited(Some(3)));
    }

    #[test]
    fn kills_process_that_outlives_timeout() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let start = Instant::now();
        let out = run_with_timeout(cmd, Duration::from_millis(300)).unwrap();
        assert_eq!(out.exit, Exit::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn keeps_output_printed_before_timeout() {
        let mut cmd = Command::new("sh");
        // exec: kill() must reach the process holding the pipe
        cmd.args(["-c", "echo partial; exec sleep 30"]);
        let out = run_with_timeout(cmd, Duration::from_millis(500)).unwrap();
        assert_eq!(out.exit, Exit::TimedOut);
        assert_eq!(out.output, "partial\n");
    }
}
