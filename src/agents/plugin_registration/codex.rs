use std::path::Path;

use anyhow::Result;

use crate::agents::CompiledPlugin;
use crate::config::Symposium;
use crate::output::Output;

pub(crate) fn sync_user_plugins(
    _sym: &Symposium,
    _root: &Path,
    _plugins: &[CompiledPlugin],
    _out: &Output,
) -> Result<bool> {
    Ok(false)
}
