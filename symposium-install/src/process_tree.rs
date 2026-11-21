//! Running a command so that stopping it stops everything it started.

use std::fs::File;
use std::io::{Read, Seek};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

#[cfg(not(windows))]
use std::process::Child;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::JobChild as Child;

#[derive(Clone, Copy)]
enum Console {
    Hidden,
    Inherited,
}

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

/// Runs `command` like [`Command::output`], polling for completion.
/// Dropping the future kills the command, together with everything it started
/// when [`own_group`] gave it a group of its own on Unix. On Windows, the child
/// is assigned to a job before it runs, so cancellation also reaches descendants
/// whose parents have exited. Background Windows acquisitions have no console.
/// Windows creation flags must be passed through `output_with_creation_flags`,
/// rather than set on `command`: stable Rust cannot read flags back from a command.
/// Windows cancellation synchronously waits with a shared five-second cleanup
/// deadline. Failures are logged; closing the job still requests termination.
pub async fn output(command: Command) -> std::io::Result<Output> {
    output_with_console(command, Console::Hidden, 0).await
}

/// Runs a Windows command with explicit creation flags in addition to the
/// temporary suspension needed for job assignment. Flags set directly on
/// `command` are replaced. Use this entry point to preserve custom flag choices.
/// Explicit `CREATE_SUSPENDED` is rejected because this API owns resuming the child.
#[cfg(windows)]
pub async fn output_with_creation_flags(command: Command, flags: u32) -> std::io::Result<Output> {
    output_with_console(command, Console::Inherited, flags).await
}

/// Keeps the caller's console for foreground installers. Windows still contains
/// the acquisition in a job; console inheritance and job membership are separate.
pub(crate) async fn output_in_foreground(command: Command) -> std::io::Result<Output> {
    output_with_console(command, Console::Inherited, 0).await
}

async fn output_with_console(
    mut command: Command,
    console: Console,
    flags: u32,
) -> std::io::Result<Output> {
    let stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    let mut running = Running {
        child: spawn(command, console, flags)?,
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

#[cfg(windows)]
fn spawn(command: Command, console: Console, flags: u32) -> std::io::Result<Child> {
    windows::JobChild::spawn(command, console, flags)
}

#[cfg(not(windows))]
fn spawn(mut command: Command, _console: Console, _flags: u32) -> std::io::Result<Child> {
    command.spawn()
}

struct Running {
    child: Child,
    exited: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.exited {
            #[cfg(windows)]
            if let Err(error) = self.child.terminate_and_wait() {
                tracing::error!(%error, "installer cancellation could not confirm process termination");
            }
            #[cfg(not(windows))]
            {
                kill(self.child.id());
                let _ = self.child.kill();
            }
            #[cfg(not(windows))]
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
