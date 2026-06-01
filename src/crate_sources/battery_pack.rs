//! Battery pack discovery via `cargo bp status --json`.
//!
//! Battery packs are crate bundles managed by the `cargo bp` tool rather than
//! declared as normal Cargo dependencies. To let plugin predicates reference
//! them by name (e.g. `crates: cli-battery-pack`), we run `cargo bp status`
//! and inject the results into the workspace crate list.

use std::path::Path;

use crate::config::Symposium;
use crate::installation::{self, CargoSource};

use super::list::WorkspaceCrate;

/// Acquire the `cargo-bp` binary (installing if needed) and run
/// `cargo bp status --json` to discover installed battery packs.
///
/// Returns one `WorkspaceCrate` per installed battery pack. These are
/// virtual entries — battery packs aren't normal Cargo dependencies, but
/// plugin predicates reference them by crate name so they must appear in
/// the workspace crate list for matching to work.
///
/// If acquisition or execution fails, returns an empty list (logged at
/// debug level) so the rest of sync proceeds unaffected.
pub async fn discover_battery_packs(sym: &Symposium, cwd: &Path) -> Vec<WorkspaceCrate> {
    let binary_path = if let Some(override_path) = sym.cargo_bp_override() {
        override_path.to_path_buf()
    } else {
        match acquire_cargo_bp(sym).await {
            Ok(path) => path,
            Err(e) => {
                tracing::debug!(error = %e, "failed to acquire cargo-bp, skipping battery pack discovery");
                return Vec::new();
            }
        }
    };

    let cwd = cwd.to_path_buf();
    let report = match tokio::task::spawn_blocking(move || {
        cargo_bp_script::StatusCommand::new()
            .program(&binary_path)
            .cwd(&cwd)
            .run()
    })
    .await
    {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "cargo bp status failed, skipping battery pack discovery");
            return Vec::new();
        }
        Err(e) => {
            tracing::debug!(error = %e, "cargo bp status task panicked, skipping battery pack discovery");
            return Vec::new();
        }
    };

    battery_packs_from_report(report)
}

/// Ensure `cargo-bp` is installed in symposium's binary cache and return the
/// path to the binary.
async fn acquire_cargo_bp(sym: &Symposium) -> anyhow::Result<std::path::PathBuf> {
    let source = CargoSource {
        crate_name: "cargo-bp".to_string(),
        version: None,
        git: None,
    };

    let acquired =
        installation::acquire_source(sym, &installation::Source::Cargo(source), Some("cargo-bp"))
            .await?;

    let binary_name = acquired
        .resolved_executable
        .unwrap_or_else(|| "cargo-bp".to_string());
    Ok(acquired
        .base
        .join("bin")
        .join(installation::platform_binary_exe(&binary_name)))
}

fn battery_packs_from_report(report: cargo_bp_script::StatusReport) -> Vec<WorkspaceCrate> {
    report
        .packs
        .into_iter()
        .filter_map(|pack| {
            semver::Version::parse(&pack.version)
                .ok()
                .map(|version| WorkspaceCrate {
                    name: pack.name,
                    version,
                    path: None,
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_packs_from_report_converts_installed_packs() {
        use cargo_bp_script::{InstalledPackStatus, ProjectInfo, StatusReport};

        let report = StatusReport::new(ProjectInfo::new("/tmp/Cargo.toml"))
            .with_pack(InstalledPackStatus::new("cli", "cli-battery-pack", "0.3.0"))
            .with_pack(InstalledPackStatus::new(
                "error",
                "error-battery-pack",
                "0.2.0",
            ));

        let crates = battery_packs_from_report(report);
        assert_eq!(crates.len(), 2);

        assert_eq!(crates[0].name, "cli-battery-pack");
        assert_eq!(crates[0].version, semver::Version::new(0, 3, 0));
        assert!(crates[0].path.is_none());

        assert_eq!(crates[1].name, "error-battery-pack");
        assert_eq!(crates[1].version, semver::Version::new(0, 2, 0));
        assert!(crates[1].path.is_none());
    }

    #[test]
    fn battery_packs_from_report_skips_unparseable_versions() {
        use cargo_bp_script::{InstalledPackStatus, ProjectInfo, StatusReport};

        let report = StatusReport::new(ProjectInfo::new("/tmp/Cargo.toml"))
            .with_pack(InstalledPackStatus::new(
                "bad",
                "bad-battery-pack",
                "not-a-version",
            ))
            .with_pack(InstalledPackStatus::new(
                "good",
                "good-battery-pack",
                "1.0.0",
            ));

        let crates = battery_packs_from_report(report);
        assert_eq!(crates.len(), 1);
        assert_eq!(crates[0].name, "good-battery-pack");
    }

    #[test]
    fn battery_packs_from_report_empty_report() {
        use cargo_bp_script::{ProjectInfo, StatusReport};

        let report = StatusReport::new(ProjectInfo::new("/tmp/Cargo.toml"));
        let crates = battery_packs_from_report(report);
        assert!(crates.is_empty());
    }
}
