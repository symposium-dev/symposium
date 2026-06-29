//! Self-update: check for newer versions and install them via `cargo install`.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::{AutoUpdate, Symposium};
use crate::output::Output;
use crate::state;
use crate::state::CURRENT_VERSION;

const CRATE_NAME: &str = "symposium";

/// Query the registry for the latest published version of symposium
/// by running `cargo search symposium --limit 1`.
pub fn latest_version(sym: &Symposium) -> Result<semver::Version> {
    let output = sym
        .cargo_command()
        .args(["search", CRATE_NAME, "--limit", "1"])
        .output()
        .context("failed to run cargo search")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo search failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_cargo_search_output(&stdout)
}

/// Parse the version from `cargo search` output.
///
/// Expected format: `symposium = "0.3.0"    # AI the Rust Way`
fn parse_cargo_search_output(output: &str) -> Result<semver::Version> {
    let version_str = output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix(&format!("{CRATE_NAME} = \"")))
        .and_then(|rest| rest.split('"').next())
        .context("unexpected cargo search output")?;

    semver::Version::parse(version_str).context("failed to parse version from cargo search")
}

/// Check whether a newer version is available.  Returns `Some(latest)`
/// if an upgrade is available, `None` if we're current or ahead.
pub fn check_upgrade(sym: &Symposium) -> Result<Option<semver::Version>> {
    let current =
        semver::Version::parse(CURRENT_VERSION).context("failed to parse current version")?;
    let latest = latest_version(sym)?;
    if latest > current {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

/// Warn-only update check for the library path (`cli::run`).
///
/// Respects config and the 24-hour throttle.  Prints a nudge when
/// `auto-update = "warn"` and a newer version exists.  Does nothing for
/// `off` or `on` — the binary handles `on` (which needs re-exec).
pub fn maybe_warn_for_update(sym: &Symposium, out: &Output) {
    if sym.config.auto_update != AutoUpdate::Warn {
        return;
    }
    if !state::should_check_for_update(sym.config_dir()) {
        return;
    }
    state::record_update_check(sym.config_dir());

    if let Ok(Some(latest)) = check_upgrade(sym) {
        out.warn(format!(
            "symposium {latest} is available (current: {CURRENT_VERSION}). \
             Run `cargo agents self-update` to upgrade.",
        ));
    }
}

/// Full update check for the binary startup path.
///
/// Respects config and the 24-hour throttle.  For `warn`, prints a
/// nudge.  For `on`, installs the update via `cargo install`, then
/// returns `true` so the caller can re-exec into the new binary.
pub async fn maybe_check_for_update(sym: &Symposium, out: &Output) -> bool {
    if sym.config.auto_update == AutoUpdate::Off {
        return false;
    }
    if !state::should_check_for_update(sym.config_dir()) {
        return false;
    }
    state::record_update_check(sym.config_dir());

    let latest = match check_upgrade(sym) {
        Ok(Some(v)) => v,
        _ => return false,
    };

    match sym.config.auto_update {
        AutoUpdate::Warn => {
            out.warn(format!(
                "symposium {latest} is available (current: {CURRENT_VERSION}). \
                 Run `cargo agents self-update` to upgrade.",
            ));
            false
        }
        AutoUpdate::On => {
            out.info(format!(
                "auto-updating symposium {CURRENT_VERSION} → {latest}..."
            ));
            match cargo_install(sym) {
                Ok(()) => {
                    out.done(format!("updated to {latest}"));
                    true
                }
                Err(e) => {
                    out.warn(format!("auto-update failed: {e}"));
                    false
                }
            }
        }
        AutoUpdate::Off => unreachable!(),
    }
}

/// Run the self-update: check for a newer version and install it.
pub fn self_update(sym: &Symposium, out: &Output) -> Result<()> {
    let target_version = match check_upgrade(sym) {
        Ok(Some(latest)) => {
            out.info(format!("updating symposium {CURRENT_VERSION} → {latest}"));
            latest
        }
        Ok(None) => {
            out.already_ok(format!("symposium {CURRENT_VERSION} is up to date"));
            return Ok(());
        }
        Err(e) => {
            out.warn(format!("could not check for updates: {e}"));
            return Err(e);
        }
    };

    cargo_install(sym)?;
    out.done(format!("updated to {target_version}"));
    Ok(())
}

fn cargo_install(sym: &Symposium) -> Result<()> {
    let status = sym
        .cargo_command()
        .args(["install", CRATE_NAME, "--force"])
        .status()
        .context("failed to run cargo install")?;

    if !status.success() {
        bail!("cargo install failed with exit code {:?}", status.code());
    }
    Ok(())
}

/// Re-execute the current process (replaces the process via exec on Unix,
/// spawn-and-exit on Windows).
pub fn re_exec() -> ! {
    let args: Vec<_> = std::env::args_os().collect();
    let program = &args[0];

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(program).args(&args[1..]).exec();
        eprintln!("failed to re-exec: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = Command::new(program)
            .args(&args[1..])
            .status()
            .unwrap_or_else(|e| {
                eprintln!("failed to re-exec: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_output_typical() {
        let output = r#"symposium = "1.2.3"    # AI the Rust Way
... and 105 crates more (use --limit N to see more)
"#;
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v, semver::Version::new(1, 2, 3));
    }

    #[test]
    fn parse_search_output_prerelease() {
        let output = "symposium = \"0.4.0-beta.1\"    # AI the Rust Way\n";
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v.to_string(), "0.4.0-beta.1");
    }

    #[test]
    fn parse_search_output_no_description() {
        let output = "symposium = \"0.3.0\"\n";
        let v = parse_cargo_search_output(output).unwrap();
        assert_eq!(v, semver::Version::new(0, 3, 0));
    }

    #[test]
    fn parse_search_output_wrong_crate() {
        let output = "something-else = \"1.0.0\"    # Not symposium\n";
        assert!(parse_cargo_search_output(output).is_err());
    }

    #[test]
    fn parse_search_output_empty() {
        assert!(parse_cargo_search_output("").is_err());
    }
}
