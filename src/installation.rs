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

/// Per-installation snapshot the dispatcher builds for the command and each
/// requirement. Drives env-var wiring for the spawned hook process:
/// `$SYMPOSIUM_DIR_<name>`, `$SYMPOSIUM_<name>`, and the `$PATH` prefix.
///
/// One layer above [`AcquiredSource`]: that's the raw source-acquisition
/// result (where the bits landed + how to resolve names within them);
/// `AcquiredInstallation` is what we keep after layering on the installation's
/// `install_commands`, `executable`/`script` resolution, and the no-source
/// case where there's nothing to acquire at all. The dispatcher only cares
/// about the resolved form, so this is what flows from
/// [`acquire_installation`] into [`build_env`] and [`build_spawn_spec`].
#[derive(Clone, Debug)]
pub struct AcquiredInstallation {
    /// Installation name as declared in the manifest. Sanitized via
    /// [`env_safe`] when used in env var keys.
    pub name: String,
    /// Cache or clone directory the source landed in. `None` for no-source
    /// installations and for global cargo (which lives in the user's
    /// `~/.cargo/bin`, outside symposium's management).
    pub base: Option<PathBuf>,
    /// What this installation resolves to at spawn time, or `None` when the
    /// installation has nothing runnable (pure setup).
    pub runnable: Option<AcquiredRunnable>,
}

/// How `Command::new` should be invoked for an installation that resolved
/// to *something* runnable.
#[derive(Clone, Debug)]
pub enum AcquiredRunnable {
    /// Symposium-resolved absolute path. Exposed as `$SYMPOSIUM_<name>` and
    /// its parent dir is prepended to `$PATH`.
    ResolvedScript { path: PathBuf },
    /// Symposium-resolved absolute path. Exposed as `$SYMPOSIUM_<name>` and
    /// its parent dir is prepended to `$PATH`.
    ResolvedExec { path: PathBuf },
    /// Bare binary name, relying on `$PATH` lookup at spawn time (global
    /// cargo). Not exposed in env vars and doesn't contribute to `$PATH` —
    /// the installation is intentionally outside symposium's view.
    GlobalExec { path: PathBuf },
}

/// Acquire an installation: run its source step (if any), run
/// `install_commands`, and resolve its runnable using the installation's
/// own `executable`/`script` plus any hook-level overrides.
///
/// `update` is forwarded to the source step: hook dispatch acquires with
/// `None` (serve cache, debounced), while the `SessionStart` prewarm uses
/// `Check` to force a refresh of git/cargo sources once per session.
///
/// `plugin_dir` is the directory containing the plugin's manifest;
/// relative `executable` / `script` paths on no-source installations
/// resolve against it.
pub async fn acquire_installation(
    sym: &Symposium,
    installation: &Installation,
    override_executable: Option<&str>,
    override_script: Option<&str>,
    update: symposium_install::UpdateLevel,
) -> anyhow::Result<AcquiredInstallation> {
    let exec_choice = installation.executable.as_deref().or(override_executable);
    let script_choice = installation.script.as_deref().or(override_script);

    let acquired = match &installation.source {
        Some(source) => {
            Some(acquire_source(&sym.install_context(), source, exec_choice, update).await?)
        }
        None => None,
    };

    run_install_commands(&installation.install_commands).await?;

    let runnable = match (&acquired, exec_choice, script_choice) {
        // Unmanaged source (global cargo): bare name, $PATH lookup at spawn.
        // Validation guarantees `exec_choice` is set for cargo + global.
        (Some(a), _, _) if a.base.is_none() => {
            let name = exec_choice
                .or(a.resolved_executable.as_deref())
                .expect("global cargo validation enforces an executable name")
                .to_string();
            Some(AcquiredRunnable::GlobalExec {
                path: PathBuf::from(name),
            })
        }
        (Some(a), Some(name), None) => Some(AcquiredRunnable::ResolvedExec {
            path: a.executable_named(name),
        }),
        (Some(a), None, Some(name)) => Some(AcquiredRunnable::ResolvedScript {
            path: a.script_named(name),
        }),
        (Some(a), None, None) if let Some(path) = a.executable() => {
            Some(AcquiredRunnable::ResolvedExec { path })
        }
        (Some(_), None, None) => None,
        // For both of these: if there is no source, then the exec/script must *obviously* be global
        (None, Some(name), None) => Some(AcquiredRunnable::ResolvedExec {
            path: PathBuf::from(name),
        }),
        (None, None, Some(name)) => Some(AcquiredRunnable::ResolvedScript {
            path: PathBuf::from(name),
        }),
        (None, None, None) => None,
        (_, Some(_), Some(_)) => unreachable!("validation forbids both executable and script"),
    };

    if let Some(AcquiredRunnable::ResolvedExec { path }) = &runnable {
        make_executable(path).ok();
    }

    Ok(AcquiredInstallation {
        name: installation.name.clone(),
        base: acquired.as_ref().and_then(|a| a.base.clone()),
        runnable,
    })
}

/// Refresh an installation's already-acquired source in place (a freshness
/// `Check`), running its `install_commands` only when a refresh actually ran.
/// A source that was never acquired — or a no-source installation — is left
/// untouched. This backs the `SessionStart` prewarm: it keeps installed hook
/// tools current without eagerly installing ones a hook may never use (those
/// install lazily on first dispatch).
pub async fn refresh_installation_if_present(
    sym: &Symposium,
    installation: &Installation,
    override_executable: Option<&str>,
) -> anyhow::Result<()> {
    let Some(source) = &installation.source else {
        return Ok(());
    };
    let exec_choice = installation.executable.as_deref().or(override_executable);
    let refreshed =
        symposium_install::refresh_source_if_present(&sym.install_context(), source, exec_choice)
            .await?;
    if refreshed {
        run_install_commands(&installation.install_commands).await?;
    }
    Ok(())
}

pub fn resolve_runnable(installation: AcquiredInstallation, label: &str) -> Result<Runnable> {
    let Some(runnable) = installation.runnable else {
        bail!("{label} has no executable or script");
    };
    let runnable = match runnable {
        AcquiredRunnable::ResolvedScript { path } => Runnable::Script(path),
        AcquiredRunnable::ResolvedExec { path } => Runnable::Exec(path),
        AcquiredRunnable::GlobalExec { path } => Runnable::Exec(path),
    };
    Ok(runnable)
}
