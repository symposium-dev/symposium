//! Resolved symposium directory paths.
//!
//! Plugin binaries (hooks, predicates, subcommands) use [`SymposiumDirs`] to
//! locate cache directories and construct [`WorkspaceDeps`](crate::pm::WorkspaceDeps)
//! with the correct cargo override and disk-cache path.

use std::env;
use std::path::{Path, PathBuf};

use crate::pm::WorkspaceDeps;

/// Resolved directory paths for the symposium installation.
///
/// Construct via [`from_environment()`](Self::from_environment) in plugin
/// binaries, or obtain from the main `Symposium` struct via `.dirs()`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SymposiumDirs {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub cargo_override: Option<PathBuf>,
}

impl SymposiumDirs {
    /// Construct with explicit paths (used by the main crate's test constructor).
    pub fn new(config_dir: PathBuf, cache_dir: PathBuf, cargo_override: Option<PathBuf>) -> Self {
        Self {
            config_dir,
            cache_dir,
            cargo_override,
        }
    }

    /// Resolve paths from environment variables.
    ///
    /// Resolution order for config dir:
    /// 1. `SYMPOSIUM_HOME` env var
    /// 2. `XDG_CONFIG_HOME/symposium`
    /// 3. `~/.symposium`
    ///
    /// Cache dir: `SYMPOSIUM_HOME/cache` → `XDG_CACHE_HOME/symposium` → `<config_dir>/cache`.
    ///
    /// Cargo override: `SYMPOSIUM_CARGO` env var.
    pub fn from_environment() -> Self {
        let config_dir = resolve_config_dir();
        let cache_dir = resolve_cache_dir(&config_dir);
        let cargo_override = env::var("SYMPOSIUM_CARGO").ok().map(PathBuf::from);

        Self {
            config_dir,
            cache_dir,
            cargo_override,
        }
    }

    /// Create a [`WorkspaceDeps`] with disk caching enabled and the
    /// correct cargo override.
    pub fn workspace_deps(&self, cwd: &Path) -> WorkspaceDeps {
        WorkspaceDeps::new(
            cwd,
            symposium_pm_cargo::CargoPmConfig {
                cache_dir: Some(self.cache_dir.clone()),
                cargo: self.cargo_override.clone(),
            },
        )
    }
}

pub(crate) fn env_dir(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub(crate) fn xdg_dir(name: &str) -> Option<PathBuf> {
    env_dir(name).filter(|path| path.is_absolute())
}

fn resolve_config_dir() -> PathBuf {
    if let Some(home) = env_dir("SYMPOSIUM_HOME") {
        home
    } else if let Some(xdg) = xdg_dir("XDG_CONFIG_HOME") {
        xdg.join("symposium")
    } else {
        dirs::home_dir()
            .expect("could not determine home directory")
            .join(".symposium")
    }
}

fn resolve_cache_dir(config_dir: &Path) -> PathBuf {
    if let Some(home) = env_dir("SYMPOSIUM_HOME") {
        return home.join("cache");
    }
    if let Some(xdg) = xdg_dir("XDG_CACHE_HOME") {
        return xdg.join("symposium");
    }
    config_dir.join("cache")
}
