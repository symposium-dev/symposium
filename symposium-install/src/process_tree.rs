//! Running a command so that stopping it stops everything it started.

use std::fs::File;
use std::io::{Read, Seek};
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

/// Puts `command` in a process group of its own on Unix, so [`kill`] reaches
/// everything it starts. Does nothing elsewhere.
#[cfg(unix)]
pub fn own_group(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    command.process_group(0)
}

/// Puts `command` in a process group of its own on Unix, so [`kill`] reaches
/// everything it starts. Does nothing elsewhere.
#[cfg(not(unix))]
pub fn own_group(command: &mut Command) -> &mut Command {
    command
}

/// Kills the process group `pid` leads on Unix, or the process tree rooted at
/// `pid` elsewhere.
#[cfg(unix)]
pub fn kill(pid: u32) {
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
}

/// Kills the process group `pid` leads on Unix, or the process tree rooted at
/// `pid` elsewhere.
#[cfg(not(unix))]
pub fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Runs `command` like [`Command::output`] without holding a thread.
/// Dropping the future kills the command with everything it started.
pub async fn output(mut command: Command) -> std::io::Result<Output> {
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    own_group(&mut command)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    let mut running = Running {
        child: command.spawn()?,
        exited: false,
    };
    let status = loop {
        if let Some(status) = running.child.try_wait()? {
            running.exited = true;
            break status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    Ok(Output {
        status,
        stdout: read_from_start(stdout)?,
        stderr: read_from_start(stderr)?,
    })
}

struct Running {
    child: Child,
    exited: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.exited {
            kill(self.child.id());
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn read_from_start(mut file: File) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.rewind()?;
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
