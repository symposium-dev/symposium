//! Symposium Recommendations
//!
//! Types and parsing for Symposium mod recommendations. This crate provides:
//!
//! - [`Recommendation`] - A recommended mod with source and conditions
//! - [`ComponentSource`] - How to obtain and run a component (cargo, npx, etc.)
//! - [`When`] - Conditions for when a recommendation applies
//!
//! # Example
//!
//! ```
//! use symposium_recommendations::{Recommendations, ComponentSource};
//!
//! let toml = r#"
//! [[recommendation]]
//! source.cargo = { crate = "sparkle-mcp", args = ["--acp"] }
//!
//! [[recommendation]]
//! source.cargo = { crate = "symposium-cargo" }
//! when.file-exists = "Cargo.toml"
//! "#;
//!
//! let recs = Recommendations::from_toml(toml).unwrap();
//! assert_eq!(recs.mods.len(), 2);
//! ```

mod source;
mod when;

pub use source::*;
pub use when::When;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// A recommendation for a component
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// The source of the component (this IS the identity)
    pub source: ComponentSource,

    /// Conditions that must be met for this recommendation to apply
    #[serde(default)]
    pub when: Option<When>,
}

impl Recommendation {
    /// Get the display name for this recommendation
    pub fn display_name(&self) -> String {
        self.source.display_name()
    }
}

/// The recommendations file format (for parsing TOML)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecommendationsFile {
    #[serde(rename = "recommendation")]
    recommendations: Vec<Recommendation>,
}

/// A collection of recommendations
#[derive(Debug, Clone)]
pub struct Recommendations {
    /// All mod recommendations
    pub mods: Vec<Recommendation>,
}

impl Recommendations {
    /// Create empty recommendations
    pub fn empty() -> Self {
        Self { mods: vec![] }
    }

    /// Parse recommendations from a TOML string
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        let file: RecommendationsFile =
            toml::from_str(toml_str).context("Failed to parse recommendations TOML")?;

        Ok(Self {
            mods: file.recommendations,
        })
    }

    /// Parse a single recommendation from a TOML string
    ///
    /// Expected format:
    /// ```toml
    /// [recommendation]
    /// source.cargo = { crate = "example" }
    /// when.file-exists = "Cargo.toml"
    /// ```
    pub fn parse_single(toml_str: &str) -> Result<Recommendation> {
        #[derive(Deserialize)]
        struct SingleFile {
            recommendation: Recommendation,
        }

        let file: SingleFile =
            toml::from_str(toml_str).context("Failed to parse recommendation TOML")?;

        Ok(file.recommendation)
    }

    /// Concatenate multiple TOML recommendation files into one
    ///
    /// Each input should be a single `[recommendation]` block.
    /// Output is a valid recommendations TOML with `[[recommendation]]` array.
    pub fn concatenate_files(files: &[&str]) -> Result<String> {
        let mut all_recs = Vec::new();

        for content in files {
            let rec = Self::parse_single(content)?;
            all_recs.push(rec);
        }

        Self::to_toml(&Recommendations { mods: all_recs })
    }

    /// Serialize recommendations to compact TOML format.
    ///
    /// Produces output like:
    /// ```toml
    /// [[recommendation]]
    /// source.cargo = { crate = "sparkle-mcp", args = ["--acp"] }
    ///
    /// [[recommendation]]
    /// source.cargo = { crate = "symposium-cargo" }
    /// when.file-exists = "Cargo.toml"
    /// ```
    pub fn to_toml(&self) -> Result<String> {
        let mut output = String::new();

        for (i, rec) in self.mods.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str("[[recommendation]]\n");
            output.push_str(&serialize_recommendation(rec)?);
        }

        Ok(output)
    }
}

/// Serialize a single recommendation to compact TOML lines.
fn serialize_recommendation(rec: &Recommendation) -> Result<String> {
    let mut lines = Vec::new();

    // Serialize source as inline table
    let source_line = serialize_source(&rec.source)?;
    lines.push(source_line);

    // Serialize when conditions if present
    if let Some(when) = &rec.when {
        let when_lines = serialize_when(when)?;
        lines.extend(when_lines);
    }

    Ok(lines.join("\n") + "\n")
}

/// Serialize ComponentSource to a single line like `source.cargo = { crate = "foo" }`
fn serialize_source(source: &ComponentSource) -> Result<String> {
    // Use serde_json to get the value, then convert to TOML inline format
    let json_value = serde_json::to_value(source)?;

    if let serde_json::Value::Object(map) = json_value {
        // ComponentSource serializes as { "variant": value }
        if let Some((variant, value)) = map.into_iter().next() {
            let inline = json_to_toml_inline(&value);
            return Ok(format!("source.{} = {}", variant, inline));
        }
    }

    anyhow::bail!("Unexpected source format")
}

/// Serialize When conditions to lines like `when.file-exists = "Cargo.toml"`
fn serialize_when(when: &When) -> Result<Vec<String>> {
    let mut lines = Vec::new();

    if let Some(path) = &when.file_exists {
        lines.push(format!(
            "when.file-exists = \"{}\"",
            escape_toml_string(path)
        ));
    }

    if let Some(paths) = &when.files_exist {
        let arr = paths
            .iter()
            .map(|p| format!("\"{}\"", escape_toml_string(p)))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("when.files-exist = [{}]", arr));
    }

    if let Some(crate_name) = &when.using_crate {
        lines.push(format!(
            "when.using-crate = \"{}\"",
            escape_toml_string(crate_name)
        ));
    }

    if let Some(crate_names) = &when.using_crates {
        let arr = crate_names
            .iter()
            .map(|c| format!("\"{}\"", escape_toml_string(c)))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("when.using-crates = [{}]", arr));
    }

    if let Some(conditions) = &when.any {
        let arr = conditions
            .iter()
            .map(|w| serialize_when_inline(w))
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        lines.push(format!("when.any = [{}]", arr));
    }

    if let Some(conditions) = &when.all {
        let arr = conditions
            .iter()
            .map(|w| serialize_when_inline(w))
            .collect::<Result<Vec<_>>>()?
            .join(", ");
        lines.push(format!("when.all = [{}]", arr));
    }

    Ok(lines)
}

/// Serialize a When as an inline table for use in arrays
fn serialize_when_inline(when: &When) -> Result<String> {
    let json_value = serde_json::to_value(when)?;
    Ok(json_to_toml_inline(&json_value))
}

/// Convert a serde_json Value to TOML inline format
fn json_to_toml_inline(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("\"{}\"", escape_toml_string(s)),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(json_to_toml_inline).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} = {}", k, json_to_toml_inline(v)))
                .collect();
            format!("{{ {} }}", pairs.join(", "))
        }
    }
}

/// Escape special characters in a TOML string
fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_recommendations() {
        let toml = r#"
[[recommendation]]
source.cargo = { crate = "sparkle-mcp", args = ["--acp"] }

[[recommendation]]
source.cargo = { crate = "symposium-cargo" }
when.file-exists = "Cargo.toml"
"#;

        let recs = Recommendations::from_toml(toml).unwrap();
        assert_eq!(recs.mods.len(), 2);
        assert_eq!(recs.mods[0].display_name(), "sparkle-mcp");
        assert!(recs.mods[0].when.is_none());
        assert!(recs.mods[1].when.is_some());
    }

    #[test]
    fn test_parse_single_recommendation() {
        let toml = r#"
[recommendation]
source.cargo = { crate = "example-mod" }
when.file-exists = "package.json"
"#;

        let rec = Recommendations::parse_single(toml).unwrap();
        assert_eq!(rec.display_name(), "example-mod");
        assert_eq!(
            rec.when.as_ref().unwrap().file_exists,
            Some("package.json".to_string())
        );
    }

    #[test]
    fn test_concatenate_files() {
        let file1 = r#"
[recommendation]
source.cargo = { crate = "mod-a" }
"#;

        let file2 = r#"
[recommendation]
source.cargo = { crate = "mod-b" }
when.file-exists = "Cargo.toml"
"#;

        let combined = Recommendations::concatenate_files(&[file1, file2]).unwrap();
        let recs = Recommendations::from_toml(&combined).unwrap();
        assert_eq!(recs.mods.len(), 2);
    }

    #[test]
    fn test_to_toml_compact_format() {
        use expect_test::expect;

        let file1 = r#"
[recommendation]
source.cargo = { crate = "sparkle-mcp", args = ["--acp"] }
"#;

        let file2 = r#"
[recommendation]
source.cargo = { crate = "symposium-cargo" }
when.file-exists = "Cargo.toml"
"#;

        let file3 = r#"
[recommendation]
source.cargo = { crate = "symposium-rust-analyzer" }
when.file-exists = "Cargo.toml"
"#;

        let combined = Recommendations::concatenate_files(&[file1, file2, file3]).unwrap();

        expect![[r#"
            [[recommendation]]
            source.cargo = { args = ["--acp"], crate = "sparkle-mcp" }

            [[recommendation]]
            source.cargo = { crate = "symposium-cargo" }
            when.file-exists = "Cargo.toml"

            [[recommendation]]
            source.cargo = { crate = "symposium-rust-analyzer" }
            when.file-exists = "Cargo.toml"
        "#]]
        .assert_eq(&combined);

        // Verify it parses back correctly
        let recs = Recommendations::from_toml(&combined).unwrap();
        assert_eq!(recs.mods.len(), 3);
        assert_eq!(recs.mods[0].display_name(), "sparkle-mcp");
        assert_eq!(recs.mods[1].display_name(), "symposium-cargo");
        assert_eq!(recs.mods[2].display_name(), "symposium-rust-analyzer");
    }
}
