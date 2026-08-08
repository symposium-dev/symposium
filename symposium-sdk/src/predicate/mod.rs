//! Types for custom predicate output.
//!
//! A custom predicate binary communicates its result via:
//! - **Exit code**: 0 = pass, non-zero = fail.
//! - **Stdout**: a JSON Lines stream of [`CustomPredicateEvent`] records. See
//!   the predicate-caching RFD for how Symposium uses these events to cache
//!   predicate results.
//!
//! Use [`PredicateEmitter`] to write records from a Rust predicate binary.
//!
//! The predicate *syntax*: the [`Predicate`] tree, its two surface spellings,
//! parsing and serde: lives in [`syntax`] and is re-exported here. Evaluating
//! a predicate needs the workspace dependency graph and the live environment,
//! so that stays in Symposium; a package manager only ever needs to carry
//! predicates through a manifest, not check them.

use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod syntax;

pub use syntax::{
    BUILTIN_PREDICATE_NAMES, DependsOnList, Predicate, PredicateSet, parse_comma_separated,
    parse_dep_atom, parse_predicate, validate_custom_predicate_name,
};

/// A record emitted by a custom predicate on stdout. Each event describes an
/// input whose change should invalidate the predicate's cached result.
///
/// The variants are intentionally granular so new watch kinds can be added
/// without breaking existing predicate binaries. Older Symposium versions
/// ignore unknown records; older predicates that emit no events are treated as
/// having no changing inputs and are cached indefinitely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum CustomPredicateEvent {
    /// The predicate's result depends on the contents of this file. Symposium
    /// invalidates the cached result if the file's fingerprint changes.
    WatchFile(PathBuf),

    /// The predicate's result depends on the value of this environment
    /// variable. Symposium fingerprints the value at read time.
    WatchEnv(String),

    /// The predicate's result becomes stale after this many milliseconds.
    /// `WatchTime(0)` disables caching entirely.
    WatchTime(u64),

    /// A crate named by a custom predicate's witness output. Retained for
    /// backward compatibility with the retired `source = "crate"` skill
    /// resolution; currently ignored by Symposium.
    SelectedCrate(SelectedCrate),
}

/// A crate named by a custom predicate's witness output. Retained for
/// backward compatibility with the retired `source = "crate"` skill
/// resolution; currently ignored by Symposium.
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

    /// Emit a raw [`CustomPredicateEvent`]. Prefer the typed helpers below.
    pub fn emit(&mut self, event: &CustomPredicateEvent) -> io::Result<&mut Self> {
        let line = serde_json::to_string(event)
            .expect("PredicateEmitter record serialization is infallible");
        writeln!(self.writer, "{line}")?;
        Ok(self)
    }

    /// Declare that the predicate's result depends on `path`. Symposium
    /// invalidates the cached result when the file's fingerprint changes.
    pub fn watch_file(&mut self, path: impl Into<PathBuf>) -> io::Result<&mut Self> {
        self.emit(&CustomPredicateEvent::WatchFile(path.into()))
    }

    /// Declare that the predicate's result depends on the value of the given
    /// environment variable.
    pub fn watch_env(&mut self, name: impl Into<String>) -> io::Result<&mut Self> {
        self.emit(&CustomPredicateEvent::WatchEnv(name.into()))
    }

    /// Declare that the predicate's result is only valid for `millis`
    /// milliseconds. `watch_time(0)` disables caching.
    pub fn watch_time(&mut self, millis: u64) -> io::Result<&mut Self> {
        self.emit(&CustomPredicateEvent::WatchTime(millis))
    }

    /// Historically caused Symposium to fetch `name@version` for `source =
    /// "crate"` skill groups; that resolution was retired, so this record is
    /// currently ignored.
    pub fn selected_crate(
        &mut self,
        name: &str,
        version: &semver::Version,
    ) -> io::Result<&mut Self> {
        self.emit(&CustomPredicateEvent::SelectedCrate(SelectedCrate {
            crate_name: name.to_string(),
            version: version.clone(),
        }))
    }
}
