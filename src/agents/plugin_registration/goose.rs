use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::agents::{Agent, CompiledPlugin};
use crate::config::Symposium;
use crate::output::Output;

pub(crate) fn sync_user_plugins(
    sym: &Symposium,
    _root: &Path,
    plugins: &[CompiledPlugin],
    _out: &Output,
) -> Result<bool> {
    super::sync_plugin_copies(sym, Agent::Goose, &user_plugins_dir(sym), plugins);
    Ok(true)
}

/// `GOOSE_PATH_ROOT` relocates this directory; `XDG_CONFIG_HOME` moves Goose's
/// `config.yaml` but not its plugins.
fn user_plugins_dir(sym: &Symposium) -> PathBuf {
    sym.env_dir("GOOSE_PATH_ROOT")
        .unwrap_or_else(|| sym.home_dir().to_path_buf())
        .join(".agents")
        .join("plugins")
}

#[cfg(test)]
mod tests;
