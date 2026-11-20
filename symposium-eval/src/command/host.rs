//! What the runner finds on the machine it runs on: tools on its `PATH`, and the active
//! Rust toolchain.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};

use crate::run::srt::Srt;

/// The first file called `name` in a directory `path` lists.
pub(crate) fn on_path(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The first file called `name` on `path`, or an error naming it.
pub(crate) fn require(path: &OsStr, name: &str) -> anyhow::Result<PathBuf> {
    on_path(path, name).with_context(|| format!("`{name}` is not on PATH"))
}

/// The installed `srt`, with the `node` that runs it and the `bwrap` around it. npm
/// links `srt` on `PATH` to its package's `dist/cli.js`.
pub(crate) fn srt(path: &OsStr) -> anyhow::Result<Srt> {
    let cli = require(path, "srt")?;
    let cli = std::fs::canonicalize(&cli)
        .with_context(|| format!("cannot resolve `{}`", cli.display()))?;
    Ok(Srt::new(
        &require(path, "node")?,
        &cli,
        &require(path, "bwrap")?,
    )?)
}

/// The active toolchain's sysroot, as `rustc` reports it.
pub(crate) fn sysroot() -> anyhow::Result<PathBuf> {
    rustc(&["--print", "sysroot"]).map(PathBuf::from)
}

/// `rustc --version`, recorded with each result.
pub(crate) fn toolchain() -> anyhow::Result<String> {
    rustc(&["--version"])
}

/// The operating system and architecture, as each result records them.
pub(crate) fn os() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn rustc(args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("rustc")
        .args(args)
        .output()
        .context("cannot run `rustc`")?;
    if !output.status.success() {
        bail!("`rustc {}` failed", args.join(" "));
    }
    Ok(String::from_utf8(output.stdout)
        .context("`rustc` printed something other than UTF-8")?
        .trim()
        .to_owned())
}

/// `path` resolved, so what is staged and hashed is the file a link names.
pub(crate) fn resolved(path: &Path) -> anyhow::Result<PathBuf> {
    std::fs::canonicalize(path).with_context(|| format!("cannot resolve `{}`", path.display()))
}
