//! Parse `[package.metadata.symposium]` from crate `Cargo.toml` files.
//!
//! Crate authors embed skill layout metadata in their `Cargo.toml` so that
//! Symposium knows where to find skills (or which other crate to redirect to).

use std::path::Path;

use anyhow::{Result, bail};
use serde::Deserialize;

/// Where a crate's skills live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// Skills are in a subdirectory of this crate's source.
    Path(String),
    /// Skills are provided by another crate (redirect).
    Crate {
        name: String,
        version: Option<String>,
    },
}

/// Parsed `[package.metadata.symposium]` section.
#[derive(Debug, Clone)]
pub struct CrateSkillMetadata {
    pub skills: Vec<SkillSource>,
}

/// Parse the `[package.metadata.symposium]` section from a Cargo.toml file.
///
/// Returns:
/// - `Ok(None)` if there is no `[package.metadata.symposium]` section.
/// - `Ok(Some(meta))` if the section is present (even if `skills = []`).
/// - `Err(...)` on malformed entries.
pub fn parse_crate_metadata(cargo_toml_path: &Path) -> Result<Option<CrateSkillMetadata>> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", cargo_toml_path.display()))?;
    parse_crate_metadata_str(&content)
}

/// Parse from a TOML string (for testing).
pub(crate) fn parse_crate_metadata_str(content: &str) -> Result<Option<CrateSkillMetadata>> {
    let doc: CargoToml = toml::from_str(content)?;

    let Some(package) = doc.package else {
        return Ok(None);
    };
    let Some(metadata) = package.metadata else {
        return Ok(None);
    };
    let Some(symposium) = metadata.symposium else {
        return Ok(None);
    };

    let mut skills = Vec::new();
    for (i, entry) in symposium.skills.iter().enumerate() {
        match (&entry.path, &entry.crate_ref) {
            (Some(p), None) => skills.push(SkillSource::Path(p.clone())),
            (None, Some(cr)) => skills.push(SkillSource::Crate {
                name: cr.name.clone(),
                version: cr.version.clone(),
            }),
            (Some(_), Some(_)) => {
                bail!("skills entry {i}: `path` and `crate` are mutually exclusive")
            }
            (None, None) => {
                bail!("skills entry {i}: must specify either `path` or `crate`")
            }
        }
    }

    Ok(Some(CrateSkillMetadata { skills }))
}

// --- serde types for Cargo.toml structure ---

#[derive(Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    metadata: Option<PackageMetadata>,
    #[serde(flatten)]
    _rest: toml::Table,
}

#[derive(Deserialize)]
struct PackageMetadata {
    symposium: Option<SymposiumMetadata>,
    #[serde(flatten)]
    _rest: toml::Table,
}

#[derive(Deserialize)]
struct SymposiumMetadata {
    #[serde(default)]
    skills: Vec<SkillEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillEntry {
    #[serde(default)]
    path: Option<String>,
    #[serde(default, rename = "crate")]
    crate_ref: Option<CrateRef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CrateRef {
    name: String,
    #[serde(default)]
    version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn parse_missing_metadata() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"
        "#};
        assert!(parse_crate_metadata_str(toml).unwrap().is_none());
    }

    #[test]
    fn parse_empty_skills() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [package.metadata.symposium]
            skills = []
        "#};
        let meta = parse_crate_metadata_str(toml).unwrap().unwrap();
        assert!(meta.skills.is_empty());
    }

    #[test]
    fn parse_path_entry() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            path = "guidance"
        "#};
        let meta = parse_crate_metadata_str(toml).unwrap().unwrap();
        assert_eq!(meta.skills.len(), 1);
        assert_eq!(meta.skills[0], SkillSource::Path("guidance".to_string()));
    }

    #[test]
    fn parse_crate_entry() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            crate = { name = "foo" }
        "#};
        let meta = parse_crate_metadata_str(toml).unwrap().unwrap();
        assert_eq!(meta.skills.len(), 1);
        assert_eq!(
            meta.skills[0],
            SkillSource::Crate {
                name: "foo".to_string(),
                version: None,
            }
        );
    }

    #[test]
    fn parse_crate_entry_with_version() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            crate = { name = "foo", version = ">=1.0" }
        "#};
        let meta = parse_crate_metadata_str(toml).unwrap().unwrap();
        assert_eq!(meta.skills.len(), 1);
        assert_eq!(
            meta.skills[0],
            SkillSource::Crate {
                name: "foo".to_string(),
                version: Some(">=1.0".to_string()),
            }
        );
    }

    #[test]
    fn parse_rejects_both_path_and_crate() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            path = "skills"
            crate = { name = "foo" }
        "#};
        let err = parse_crate_metadata_str(toml).unwrap_err();
        assert!(err.to_string().contains("mutually exclusive"), "got: {err}");
    }

    #[test]
    fn parse_rejects_neither_path_nor_crate() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
        "#};
        let err = parse_crate_metadata_str(toml).unwrap_err();
        assert!(
            err.to_string().contains("must specify either"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_multiple_entries() {
        let toml = indoc! {r#"
            [package]
            name = "my-crate"
            version = "0.1.0"
            edition = "2021"

            [[package.metadata.symposium.skills]]
            path = "guidance"

            [[package.metadata.symposium.skills]]
            crate = { name = "bar", version = "^2.0" }
        "#};
        let meta = parse_crate_metadata_str(toml).unwrap().unwrap();
        assert_eq!(meta.skills.len(), 2);
        assert_eq!(meta.skills[0], SkillSource::Path("guidance".to_string()));
        assert_eq!(
            meta.skills[1],
            SkillSource::Crate {
                name: "bar".to_string(),
                version: Some("^2.0".to_string()),
            }
        );
    }
}
