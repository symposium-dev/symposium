//! Shared installation resolution for hooks and subcommands.
//!
//! Wraps `symposium_install` primitives with main-crate types (`Symposium`,
//! `plugins::Installation`) so both hook dispatch and subcommand dispatch can
//! resolve an installation to a runnable without duplicating the logic.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::config::Symposium;
use crate::plugins::Installation;
use symposium_install::{Runnable, acquire_source, make_executable};

/// Run a list of post-install shell commands sequentially. Stops at the first
/// failure.
pub async fn run_install_commands(commands: &[String]) -> Result<()> {
    for cmd in commands {
        let status = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .status()
            .await?;
        if !status.success() {
            bail!("install command `{cmd}` exited with {status}");
        }
    }
    Ok(())
}

/// Acquire an installation as a requirement: run its kind-specific source
/// step (if any), then any declared `install_commands`. Does NOT resolve to
/// a runnable — requirements are only ever "ensure on disk".
pub async fn install_requirement(sym: &Symposium, install: &Installation) -> Result<()> {
    if let Some(source) = &install.source {
        acquire_source(&sym.install_context(), source, install.executable.as_deref()).await?;
    }
    run_install_commands(&install.install_commands).await
}

/// Acquire an installation's source (if any), run its `install_commands`, and
/// pick a `Runnable` from (`installation.executable`/`script`) with the
/// caller's overrides used when the installation leaves them unset.
///
/// `label` is the caller's identifier for error messages (e.g. `"hook `foo`"`
/// or `"subcommand `bar`"`). Used purely for diagnostics; does not affect
/// resolution.
///
/// Validation (in `plugins.rs`) guarantees that across the installation and
/// the caller-side overrides, at most one of `executable`/`script` is set,
/// so this function does not re-check that invariant.
pub async fn resolve_runnable(
    sym: &Symposium,
    installation: &Installation,
    override_executable: Option<&str>,
    override_script: Option<&str>,
    label: &str,
) -> Result<Runnable> {
    let exec_choice = installation.executable.as_deref().or(override_executable);
    let script_choice = installation.script.as_deref().or(override_script);

    let acquired = match &installation.source {
        Some(source) => Some(acquire_source(&sym.install_context(), source, exec_choice).await?),
        None => None,
    };

    run_install_commands(&installation.install_commands).await?;

    let runnable = match (acquired, exec_choice, script_choice) {
        (Some(a), Some(name), None) => Runnable::Exec(a.executable_named(name)),
        (Some(a), None, Some(name)) => Runnable::Script(a.script_named(name)),
        (Some(a), None, None) => {
            if let Some(name) = a.resolved_executable.as_deref() {
                Runnable::Exec(a.executable_named(name))
            } else {
                bail!("{label}: command resolved to no executable or script");
            }
        }
        (None, Some(name), None) => Runnable::Exec(PathBuf::from(name)),
        (None, None, Some(name)) => Runnable::Script(PathBuf::from(name)),
        (None, None, None) => bail!("{label}: command resolved to no executable or script"),
        (_, Some(_), Some(_)) => unreachable!("validation forbids both executable and script"),
    };

    match &runnable {
        Runnable::Exec(path) => {
            make_executable(path).ok();
        }
        Runnable::Script(_) => {}
        _ => bail!("{label}: unsupported runnable kind"),
    }
    Ok(runnable)
}
