//! Types for custom predicate output.
//!
//! A custom predicate binary communicates its result via:
//! - **Exit code**: 0 = pass, non-zero = fail.
//! - **Stdout** (optional): JSON Lines output, one record per line, naming
//!   crates that should be fetched for `source = "crate"` skill resolution.
//!
//! Each line is a tagged JSON object with exactly one key identifying the
//! record type. Unknown record types are warned and skipped (forward
//! compatibility). Malformed lines cause the predicate to be treated as
//! failed.
//!
//! Use [`PredicateEmitter`] to write records from a Rust predicate binary.

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

/// A crate selected by a custom predicate's witness output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedCrate {
    pub crate_name: String,
    pub version: semver::Version,
}

impl Serialize for SelectedCrate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("SelectedCrate", 2)?;
        s.serialize_field("name", &self.crate_name)?;
        s.serialize_field("version", &self.version.to_string())?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for SelectedCrate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "name")]
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

/// Emits predicate output records to stdout (or any writer) in JSON Lines format.
///
/// Each call to a method like [`selected_crate`](PredicateEmitter::selected_crate)
/// writes one line to the underlying writer.
///
/// # Example
///
/// ```no_run
/// use symposium_sdk::predicate::PredicateEmitter;
///
/// PredicateEmitter::stdout()
///     .selected_crate("serde", &semver::Version::new(1, 0, 217)).unwrap()
///     .selected_crate("tokio", &semver::Version::new(1, 40, 0)).unwrap();
/// ```
pub struct PredicateEmitter<W: Write> {
    writer: W,
}

impl PredicateEmitter<io::Stdout> {
    pub fn stdout() -> Self {
        Self {
            writer: io::stdout(),
        }
    }
}

impl<W: Write> PredicateEmitter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Emitting this record causes Symposium to fetch `name@version` as a
    /// crate source for `source = "crate"` skill groups.
    pub fn selected_crate(
        &mut self,
        name: &str,
        version: &semver::Version,
    ) -> io::Result<&mut Self> {
        #[derive(Serialize)]
        struct Record<'a> {
            #[serde(rename = "selectedCrate")]
            selected_crate: Inner<'a>,
        }
        #[derive(Serialize)]
        struct Inner<'a> {
            name: &'a str,
            version: String,
        }
        let record = Record {
            selected_crate: Inner {
                name,
                version: version.to_string(),
            },
        };
        let line = serde_json::to_string(&record)
            .expect("PredicateEmitter record serialization is infallible");
        writeln!(self.writer, "{line}")?;
        Ok(self)
    }
}
