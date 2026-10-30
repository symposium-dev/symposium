//! Shared installation resolution for hooks and subcommands.
//!
//! Wraps `symposium_install` primitives with main-crate types (`Symposium`,
//! `plugins::Installation`) so both hook dispatch and subcommand dispatch can
//! resolve an installation to a runnable without duplicating the logic.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Instant;

use anyhow::{Result, bail};

use crate::config::Symposium;
use crate::plugins::Installation;
use symposium_install::{Runnable, acquire_source, make_executable};

/// Run a list of post-install shell commands sequentially. Stops at the first
/// failure.
///
/// Their stdout goes to stderr: these commands also run while dispatching a
/// hook, where stdout carries the agent's protocol.
pub async fn run_install_commands(commands: &[String], deadline: Option<Instant>) -> Result<()> {
    for cmd in commands {
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd).stdout(std::io::stderr());
        let status = match deadline {
            Some(deadline) => status_by(command, deadline).await?,
            None => Some(tokio::process::Command::from(command).status().await?),
        };
        let Some(status) = status else {
            bail!("install command `{cmd}` ran past the hook time budget");
        };
        if !status.success() {
            bail!("install command `{cmd}` exited with {status}");
        }
    }
    Ok(())
}

async fn status_by(
    mut command: std::process::Command,
    deadline: Instant,
) -> std::io::Result<Option<ExitStatus>> {
    crate::process_tree::own_group(&mut command);
    let mut child = tokio::process::Command::from(command).spawn()?;
    match tokio::time::timeout_at(deadline.into(), child.wait()).await {
        Ok(status) => status.map(Some),
        Err(_) => {
            if let Some(pid) = child.id() {
                crate::process_tree::kill(pid);
            }
            let _ = child.kill().await;
            Ok(None)
        }
    }
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
    deadline: Option<Instant>,
) -> anyhow::Result<AcquiredInstallation> {
    let exec_choice = installation.executable.as_deref().or(override_executable);
    let script_choice = installation.script.as_deref().or(override_script);

    let acquired = match &installation.source {
        Some(source) => {
            let context = sym.install_context();
            let acquiring = acquire_source(&context, source, exec_choice, update);
            let acquired = match deadline {
                Some(deadline) => tokio::time::timeout_at(deadline.into(), acquiring)
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "acquiring `{}` ran past the hook time budget",
                            installation.name
                        )
                    })?,
                None => acquiring.await,
            };
            Some(acquired?)
        }
        None => None,
    };

    run_install_commands(&installation.install_commands, deadline).await?;

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
        run_install_commands(&installation.install_commands, None).await?;
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

#[cfg(all(test, unix))]
mod cancellation_tests {
    #[test]
    fn timed_out_acquisition_stops_cargo_installer() {
        use std::time::{Duration, Instant};

        use super::acquire_installation;
        use crate::plugins::Installation;
        use std::os::unix::fs::PermissionsExt;
        use symposium_install::{CargoSource, Source, UpdateLevel};

        // Keep these files alive until runtime shutdown has joined blocking workers.
        let signals = tempfile::tempdir().unwrap();
        let started = signals.path().join("started");
        let release = signals.path().join("release");
        let completed = signals.path().join("completed");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut sym = crate::config::Symposium::from_dir(signals.path());
        runtime.block_on(async {
        // Gate completion on the test, instead of sleeping past a production
        // timeout. No real Cargo invocation or network access occurs.
        let mock = signals.path().join("mock-cargo");
        std::fs::write(&mock, format!(
            "#!/bin/sh\necho started > '{0}/started'\nwhile [ ! -f '{0}/release' ]; do sleep 0.01; done\necho completed > '{0}/completed'\n",
            signals.path().display(),
        )).unwrap();
        std::fs::set_permissions(&mock, std::fs::Permissions::from_mode(0o755)).unwrap();
        sym.set_cargo_override(mock);
        let mut cargo = CargoSource::new("test-installer");
        cargo.global = true;
        let installation = Installation {
            name: "test-installer".into(),
            requirements: vec![],
            install_commands: vec![],
            source: Some(Source::Cargo(cargo)),
            executable: Some("test-installer".into()),
            script: None,
            args: vec![],
        };
        let result = acquire_installation(
            &sym,
            &installation,
            None,
            None,
            UpdateLevel::None,
            Some(Instant::now() + Duration::from_secs(1)),
        ).await;

        // Always release before asserting, so even a failing test lets a leaked
        // worker finish and cannot strand runtime shutdown waiting for our gate.
        std::fs::write(&release, "release").unwrap();
        assert!(started.exists(), "the fake installer must actually start");
        let error = result.expect_err("acquisition must hit its deadline");
        assert!(error.to_string().contains("time budget"), "{error:#}");
    });
        drop(runtime);

        assert!(
            !completed.exists(),
            "Cargo installer continued after acquisition timed out"
        );
    }
}
