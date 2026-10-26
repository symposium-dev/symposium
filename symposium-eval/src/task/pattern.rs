use std::fmt;

use globset::{Glob, GlobBuilder, GlobMatcher};
use serde::Deserialize;
use thiserror::Error;

/// A relative, `/`-separated glob such as `src/**`, compiled when the task loads so an
/// unusable pattern is an authoring error rather than a grading failure. `*` never
/// crosses a `/`, on every platform. The first component is literal, so a pattern names
/// where in the project it applies; `**` or `*.toml` would reach every directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct EditablePattern(Glob);

#[derive(Debug, Error)]
pub(crate) enum PatternError {
    #[error(
        "editable pattern `{0}` must be a relative path of names joined by single `/`, without `.` or `..`"
    )]
    NotRelative(String),
    #[error("editable pattern `{0}` must start with a literal directory or file name")]
    Unrooted(String),
    #[error("editable pattern `{pattern}` is not a valid glob")]
    Glob {
        pattern: String,
        #[source]
        source: globset::Error,
    },
}

impl EditablePattern {
    pub(crate) fn new(pattern: &str) -> Result<Self, PatternError> {
        // Tree paths have no empty, `.`, or `..` components, so a pattern with one would
        // either match nothing or reach outside the project.
        let irregular = pattern
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."));
        if irregular || pattern.contains(['\\', ':']) {
            return Err(PatternError::NotRelative(pattern.to_owned()));
        }
        let first = pattern.split('/').next().unwrap_or_default();
        if first.contains(['*', '?', '[', ']', '{', '}']) {
            return Err(PatternError::Unrooted(pattern.to_owned()));
        }
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map(Self)
            .map_err(|source| PatternError::Glob {
                pattern: pattern.to_owned(),
                source,
            })
    }

    pub(crate) fn matcher(&self) -> GlobMatcher {
        self.0.compile_matcher()
    }
}

impl TryFrom<String> for EditablePattern {
    type Error = PatternError;

    fn try_from(pattern: String) -> Result<Self, Self::Error> {
        Self::new(&pattern)
    }
}

impl AsRef<str> for EditablePattern {
    fn as_ref(&self) -> &str {
        self.0.glob()
    }
}

impl fmt::Display for EditablePattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.glob())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_relative_globs() {
        for pattern in ["src/**", "src/lib.rs", "tests/*.rs", "Cargo.toml"] {
            assert!(EditablePattern::new(pattern).is_ok(), "{pattern}");
        }
    }

    #[test]
    fn rejects_patterns_that_are_not_plain_relative_paths() {
        for pattern in [
            "",
            "/src/**",
            "../src",
            "src/../..",
            r"src\**",
            "C:/src",
            "./src/**",
            "src//**",
            "src/",
        ] {
            assert!(
                matches!(
                    EditablePattern::new(pattern),
                    Err(PatternError::NotRelative(_))
                ),
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn rejects_patterns_without_a_literal_first_component() {
        for pattern in ["**", "*.toml", "*/lib.rs", "{src,tests}/**", "s?c/**"] {
            assert!(
                matches!(
                    EditablePattern::new(pattern),
                    Err(PatternError::Unrooted(_))
                ),
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn rejects_malformed_globs() {
        assert!(matches!(
            EditablePattern::new("src/[abc"),
            Err(PatternError::Glob { .. })
        ));
    }

    #[test]
    fn star_does_not_cross_directories() {
        let matcher = EditablePattern::new("src/*.rs").unwrap().matcher();

        assert!(matcher.is_match("src/lib.rs"));
        assert!(!matcher.is_match("src/nested/lib.rs"));
    }
}
