use std::process::Command;

#[cfg(unix)]
pub fn own_group(command: &mut Command) -> &mut Command {
    use std::os::unix::process::CommandExt;
    command.process_group(0)
}

#[cfg(not(unix))]
pub fn own_group(command: &mut Command) -> &mut Command {
    command
}

#[cfg(unix)]
pub fn kill(pid: u32) {
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub fn kill(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
