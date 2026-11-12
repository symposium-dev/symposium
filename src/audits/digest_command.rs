//! `cargo agents plugin digest <crate[@version]|dir>`: print the payload digest an audits
//! file matches a crate on.
//!
//! Exists so that tooling outside Symposium (the runner that maintains an audits
//! list) hashes exactly as the client does, instead of reimplementing
//! [`payload_digest`](super::digest::payload_digest).

use std::path::Path;

use anyhow::{Context, Result, bail};
use symposium_pm_cargo::sources::normalize_crate_name;

use super::digest::payload_digest;
use crate::config::Symposium;
use crate::crate_command::{ResolvedCrate, resolve_crate};
use crate::report::{DigestOutcome, ReportEvent};

/// Compute and report the digest for `target`: an existing directory is a crate
/// root; anything else is a crate name, optionally `name@version`, resolved
/// the way `crate-info` resolves it (relative directories are taken from
/// `cwd`). A target that looks like a path (a separator, a leading `.`) or
/// names something that is not a directory is never taken for a crate name.
///
/// Fails, after reporting the reason, when the payload is not eligible for a
/// digest or the crate cannot be resolved.
pub async fn run(sym: &Symposium, cwd: &Path, target: &str) -> Result<()> {
    let dir = cwd.join(target);
    let looks_like_path = target.starts_with('.') || target.contains(['/', '\\']);
    let (root, krate) = if dir.is_dir() {
        (dir, None)
    } else if looks_like_path || dir.exists() {
        bail!("`{target}` is not a directory");
    } else {
        let found = resolve(sym, cwd, target).await?;
        (
            found.root,
            Some(format!("{} {}", found.name, found.version)),
        )
    };
    let result = payload_digest(&root);
    let outcome = match &result {
        Ok(digest) => DigestOutcome::Digest(*digest),
        Err(why) => DigestOutcome::Ineligible(why.to_string()),
    };
    tracing::info!(
        report = %ReportEvent::PayloadDigest {
            target: target.to_string(),
            krate,
            outcome,
        },
    );
    if result.is_err() {
        bail!("the payload of `{target}` is not eligible for a digest");
    }
    Ok(())
}

/// Resolve `name` or `name@version` to a crate's source. A version names one
/// exact release: the workspace's own copy when the workspace resolved that
/// release (a path dependency included), otherwise what the cargo package
/// manager fetches for `=<version>`, since cargo reads a bare version as a
/// caret requirement.
async fn resolve(sym: &Symposium, cwd: &Path, target: &str) -> Result<ResolvedCrate> {
    let Some((name, version)) = target.split_once('@') else {
        return resolve_crate(sym, target, None, cwd).await;
    };
    let version = semver::Version::parse(version)
        .with_context(|| format!("failed to read the version in `{target}`"))?;
    let normalized = normalize_crate_name(name);
    let deps = sym.workspace_deps(cwd);
    if let Some((krate, dir)) = deps
        .member_crates()
        .iter()
        .chain(deps.crates())
        .filter(|c| normalize_crate_name(&c.name) == normalized && c.version == version)
        .find_map(|c| Some((c, c.path.as_ref().or(c.source_dir.as_ref())?)))
    {
        return Ok(ResolvedCrate {
            name: krate.name.clone(),
            version: krate.version.to_string(),
            root: dir.clone(),
        });
    }
    resolve_crate(sym, name, Some(&format!("={version}")), cwd).await
}
