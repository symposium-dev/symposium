//! Types for custom predicate output.
//!
//! FIXME: this witness/emitter channel is currently **unused**. It was built to
//! let a custom predicate name crates to fetch for the retired `source =
//! "crate"` skill resolution; custom predicates are now a boolean gate only
//! (pass/fail via exit code), and Symposium ignores their stdout. The intended
//! future use is to let a custom predicate **set fields on the plugin (or
//! component) it gates** — contributing values back into the manifest — through
//! a channel like this. Until that lands, [`PredicateEmitter`] / [`SelectedCrate`]
//! have no effect.
//!
//! A custom predicate binary communicates its result via:
//! - **Exit code**: 0 = pass, non-zero = fail.
//! - **Stdout**: reserved for the future use above; currently ignored.
//!
//! Use [`PredicateEmitter`] to write records from a Rust predicate binary.

use std::io::{self, Write};

use serde::{Deserialize, Serialize};

/// A crate named by a custom predicate's witness output. Currently unused — see
/// the module-level FIXME.
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

    /// Historically caused Symposium to fetch `name@version` for `source =
    /// "crate"` skill groups; that resolution was retired, so this record is
    /// currently ignored (see the module-level FIXME).
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
