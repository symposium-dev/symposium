//! Reading an externally authored agent plugin package as a symposium plugin.
//!
//! A directory holding a `plugin.json` becomes a third kind of plugin entry
//! beside one holding a `SYMPOSIUM.toml` and one holding a bare `SKILL.md`, and
//! it is recognized in the same three positions: a registry entry, a workspace
//! member, and a dependency's source.
//!
//! Failures are contained to the smallest affected unit and reported rather than
//! suppressed, which is what the format requires: a manifest that breaks its
//! schema rejects that package alone, an unknown top-level field is reported and
//! ignored, and a broken skill is skipped while the rest of the package loads.

use std::path::Path;

use anyhow::{Context, Result};

use super::manifest::IncomingManifest;
use crate::plugins::{Plugin, PluginKind, PluginSource, SkillDepth, SkillGroup};
use crate::report::ReportEvent;

/// The manifest that marks a directory as an agent plugin package.
pub const MANIFEST_FILE: &str = "plugin.json";

/// The format's other component type. Symposium reads the skills half, so this
/// is reported as unsupported rather than silently ignored.
const MCP_FILE: &str = "mcp.json";

/// Fixed by the format: skills live in `skills/`, one per immediate child, and
/// the manifest cannot point somewhere else.
const SKILLS_DIR: &str = "skills";

/// Load the package in `dir`.
///
/// `gated_by_position` is true where finding the package is itself the gate — a
/// workspace member, or a crate reached through a reference. Elsewhere the
/// ordinary dormancy rule applies: the format cannot express when a package
/// applies, so one that declares no `dev.symposium` gate waits for a `use` entry
/// rather than activating everywhere.
pub fn load(dir: &Path, gated_by_position: bool) -> Result<Plugin> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest = IncomingManifest::parse(&text)
        .with_context(|| format!("invalid {}", manifest_path.display()))?;

    let unknown = manifest.unknown_fields();
    if !unknown.is_empty() {
        report_warning(format!(
            "{}: ignoring unknown field(s) {}",
            crate::output::display_path(&manifest_path),
            unknown.join(", ")
        ));
    }

    if dir.join(MCP_FILE).is_file() {
        report_warning(format!(
            "{}: MCP servers in an agent plugin are not supported yet; its skills still load",
            crate::output::display_path(&dir.join(MCP_FILE))
        ));
    }

    let extension = manifest.symposium_extension()?.unwrap_or_default();
    let predicates =
        crate::predicate::PredicateSet::merged(extension.depends_on, extension.predicates);
    let requires_use = !gated_by_position && crate::plugins::dormant_without_gate(&predicates);

    Ok(Plugin {
        name: manifest.name,
        kind: PluginKind::AgentPlugin,
        version: manifest.version,
        description: manifest.description,
        predicates,
        skills: vec![SkillGroup {
            source: PluginSource::Path(SKILLS_DIR.into()),
            depth: SkillDepth::ImmediateChildren,
            ..Default::default()
        }],
        requires_use,
        ..Default::default()
    })
}

/// Identity a `plugin.json` supplies to a `SYMPOSIUM.toml` sitting beside it.
///
/// A directory carrying both loads as a symposium plugin, since the TOML is the
/// richer manifest, but takes what the TOML leaves out from the JSON.
#[derive(Debug, Default)]
pub struct SiblingIdentity {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// Read the identity from a `plugin.json` in `dir`, if there is a usable one.
/// A malformed sibling is reported and ignored: the TOML is what defines this
/// plugin, so a broken companion must not reject it.
pub fn sibling_identity(dir: &Path) -> SiblingIdentity {
    let path = dir.join(MANIFEST_FILE);
    if !path.is_file() {
        return SiblingIdentity::default();
    }
    let parsed = std::fs::read_to_string(&path)
        .map_err(anyhow::Error::from)
        .and_then(|text| IncomingManifest::parse(&text));
    match parsed {
        Ok(manifest) => SiblingIdentity {
            name: Some(manifest.name),
            version: manifest.version,
            description: manifest.description,
        },
        Err(e) => {
            report_warning(format!(
                "{}: ignoring companion manifest: {e:#}",
                crate::output::display_path(&path)
            ));
            SiblingIdentity::default()
        }
    }
}

fn report_warning(message: String) {
    tracing::info!(report = %ReportEvent::Warning { message });
}
