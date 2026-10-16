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

fn user_plugins_dir(sym: &Symposium) -> PathBuf {
    crate::agents::goose_path_root(sym)
        .unwrap_or_else(|| sym.home_dir().to_path_buf())
        .join(".agents")
        .join("plugins")
}
