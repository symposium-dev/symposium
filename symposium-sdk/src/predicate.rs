//! Types for custom predicate output.
//!
//! A custom predicate binary communicates its result via:
//! - **Exit code**: 0 = pass, non-zero = fail.
//! - **Stdout** (optional): JSON witness output naming crates that should be
//!   fetched for `source = "crate"` skill resolution.
//!
//! If stdout is empty, the predicate passes without contributing any witness
//! crates. If stdout contains JSON, it must conform to [`PredicateOutput`].
//! Malformed JSON causes the predicate to be treated as failed.

use serde::{Deserialize, Serialize};

/// The JSON structure a custom predicate prints to stdout to report witness
/// crates.
///
/// # Example
///
/// ```
/// use symposium_sdk::predicate::{PredicateOutput, SelectedCrate};
///
/// let output = PredicateOutput {
///     selected_crates: vec![
///         SelectedCrate {
///             crate_name: "serde".into(),
///             version: semver::Version::new(1, 0, 210),
///         },
///     ],
/// };
/// assert!(serde_json::to_string(&output).unwrap().contains("selectedCrates"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PredicateOutput {
    /// The crates selected by this predicate. Each entry names a crate whose
    /// source will be fetched for skill discovery.
    pub selected_crates: Vec<SelectedCrate>,
}

/// A crate selected by a custom predicate's witness output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedCrate {
    /// The crate name (e.g. `"serde"`, `"cli-battery-pack"`).
    pub crate_name: String,
    /// The exact version to fetch.
    pub version: semver::Version,
}

impl Serialize for SelectedCrate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("SelectedCrate", 2)?;
        s.serialize_field("crate", &self.crate_name)?;
        s.serialize_field("version", &self.version.to_string())?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for SelectedCrate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "crate")]
            crate_name: String,
            version: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        let version = semver::Version::parse(&raw.version).map_err(serde::de::Error::custom)?;
        Ok(SelectedCrate {
            crate_name: raw.crate_name,
            version,
        })
    }
}

impl PredicateOutput {
    /// Create an empty output (pass, no witness crates).
    pub fn empty() -> Self {
        Self {
            selected_crates: Vec::new(),
        }
    }
}
