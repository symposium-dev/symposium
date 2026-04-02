//! Crate predicate parsing for skill matching.
//!
//! Skills declare which crates they advise on using a simple predicate syntax:
//!
//! ```text
//! serde                          -- bare crate name (any version)
//! serde>=1.0                     -- crate with version constraint
//! ```

use anyhow::{Context, Result, bail};

/// A predicate matching a single crate dependency, optionally constrained by version.
///
/// Serializes to/from its string representation (e.g., `"serde>=1.0"`).
#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub name: String,
    pub version_req: Option<semver::VersionReq>,
}

impl Predicate {
    /// Evaluate this predicate against a workspace dependency list.
    pub fn matches(&self, deps: &[(String, semver::Version)]) -> bool {
        deps.iter().any(|(dep_name, dep_ver)| {
            dep_name == &self.name
                && self
                    .version_req
                    .as_ref()
                    .map_or(true, |req| req.matches(dep_ver))
        })
    }

    /// Returns true if this predicate references the given crate name.
    pub fn references_crate(&self, name: &str) -> bool {
        self.name == name
    }

    /// Collect the crate name referenced by this predicate into a set.
    pub fn collect_crate_names(&self, out: &mut std::collections::BTreeSet<String>) {
        out.insert(self.name.clone());
    }
}

/// Parse a list of predicate strings.
///
/// Each string can be a bare crate name or a crate with version constraint.
pub fn parse_predicates(strings: &[String]) -> Result<Vec<Predicate>> {
    strings
        .iter()
        .map(|s| parse(s).with_context(|| format!("failed to parse predicate: {s:?}")))
        .collect()
}

/// Parse a comma-separated predicate string into multiple predicates.
///
/// Used for SKILL.md frontmatter where `crates: serde, tokio>=1.0` is a single line.
pub fn parse_comma_separated(input: &str) -> Result<Vec<Predicate>> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| parse(s).with_context(|| format!("failed to parse predicate: {s:?}")))
        .collect()
}

/// Parse a single predicate string.
pub fn parse(input: &str) -> Result<Predicate> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty predicate string");
    }
    let mut parser = Parser::new(input);
    let pred = parser.parse_atom()?;
    parser.skip_whitespace();
    if parser.pos < parser.input.len() {
        bail!(
            "unexpected trailing input at position {}: {:?}",
            parser.pos,
            &parser.input[parser.pos..]
        );
    }
    Ok(pred)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn parse_atom(&mut self) -> Result<Predicate> {
        self.skip_whitespace();
        let start = self.pos;

        // Consume crate name: [a-zA-Z0-9_-]+
        while self.pos < self.input.len() {
            let c = self.input.as_bytes()[self.pos];
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                self.pos += 1;
            } else {
                break;
            }
        }

        let name = &self.input[start..self.pos];
        if name.is_empty() {
            bail!(
                "expected crate name at position {}: {:?}",
                start,
                self.remaining()
            );
        }

        // Check for version constraint (starts with >=, <=, >, <, =, ^, ~).
        // Bare `=` (not `>=` or `<=`) is treated as `^` (compatible version),
        // matching Cargo's default behavior for dependency specifications.
        let version_req = if self.pos < self.input.len() {
            let next = self.input.as_bytes()[self.pos];
            if matches!(next, b'>' | b'<' | b'=' | b'^' | b'~') {
                let vstart = self.pos;
                // Consume until delimiter: whitespace or end
                while self.pos < self.input.len() {
                    let c = self.input.as_bytes()[self.pos];
                    if c.is_ascii_whitespace() {
                        break;
                    }
                    self.pos += 1;
                }
                let raw = self.input[vstart..self.pos].trim();
                // `==X.Y` → exact match (`=X.Y` in semver)
                // `=X.Y`  → compatible version (`^X.Y`), matching Cargo's default
                // `>=`, `<=`, `>`, `<`, `^`, `~` → passed through as-is
                let constraint = if raw.starts_with("==") {
                    std::borrow::Cow::Owned(format!("={}", &raw[2..]))
                } else if raw.starts_with('=') {
                    std::borrow::Cow::Owned(format!("^{}", &raw[1..]))
                } else {
                    std::borrow::Cow::Borrowed(raw)
                };
                Some(semver::VersionReq::parse(&constraint)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Predicate {
            name: name.to_string(),
            version_req,
        })
    }
}

impl serde::Serialize for Predicate {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for Predicate {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Predicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        if let Some(req) = &self.version_req {
            write!(f, "{req}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    fn workspace() -> Vec<(String, semver::Version)> {
        vec![
            ("serde".into(), v("1.0.219")),
            ("serde_json".into(), v("1.0.133")),
            ("tokio".into(), v("1.42.0")),
            ("anyhow".into(), v("1.0.95")),
        ]
    }

    // --- Parser tests ---

    #[test]
    fn parse_bare_name() {
        let p = parse("serde").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "serde".into(),
                version_req: None,
            }
        );
    }

    #[test]
    fn parse_name_with_hyphen() {
        let p = parse("tokio-stream").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "tokio-stream".into(),
                version_req: None,
            }
        );
    }

    #[test]
    fn parse_version_gte() {
        let p = parse("serde>=1.0").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "serde".into(),
                version_req: Some(semver::VersionReq::parse(">=1.0").unwrap()),
            }
        );
    }

    #[test]
    fn parse_version_caret() {
        let p = parse("tokio^1.40").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "tokio".into(),
                version_req: Some(semver::VersionReq::parse("^1.40").unwrap()),
            }
        );
    }

    #[test]
    fn parse_version_bare_eq_is_caret() {
        // `=1.2` is sugar for `^1.2` (compatible version)
        let p = parse("serde=1.2").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "serde".into(),
                version_req: Some(semver::VersionReq::parse("^1.2").unwrap()),
            }
        );
    }

    #[test]
    fn parse_version_gte_not_rewritten() {
        // `>=1.0` stays as `>=1.0`, not rewritten to `^`
        let p = parse("serde>=1.0").unwrap();
        let req = p.version_req.as_ref().unwrap();
        // >=1.0 matches 2.0, ^1.0 does not
        assert!(req.matches(&semver::Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn bare_eq_matches_compatible_versions() {
        let p = parse("serde=1.2").unwrap();
        let deps = vec![("serde".into(), v("1.3.0"))];
        assert!(
            p.matches(&deps),
            "=1.2 should match 1.3.0 (caret semantics)"
        );

        let deps_major = vec![("serde".into(), v("2.0.0"))];
        assert!(!p.matches(&deps_major), "=1.2 should not match 2.0.0");
    }

    #[test]
    fn parse_version_double_eq_is_exact() {
        // `==1.2.0` is exact match
        let p = parse("serde==1.2.0").unwrap();
        assert_eq!(
            p,
            Predicate {
                name: "serde".into(),
                version_req: Some(semver::VersionReq::parse("=1.2.0").unwrap()),
            }
        );
    }

    #[test]
    fn double_eq_exact_match_semantics() {
        let p = parse("serde==1.2.0").unwrap();

        let exact = vec![("serde".into(), v("1.2.0"))];
        assert!(p.matches(&exact), "==1.2.0 should match 1.2.0");

        let patch = vec![("serde".into(), v("1.2.1"))];
        assert!(!p.matches(&patch), "==1.2.0 should not match 1.2.1");

        let minor = vec![("serde".into(), v("1.3.0"))];
        assert!(!p.matches(&minor), "==1.2.0 should not match 1.3.0");
    }

    #[test]
    fn matches_version_gt() {
        let p = parse("serde>1.0.0").unwrap();
        let yes = vec![("serde".into(), v("1.0.1"))];
        assert!(p.matches(&yes));
        let no = vec![("serde".into(), v("1.0.0"))];
        assert!(!p.matches(&no));
    }

    #[test]
    fn matches_version_lt() {
        let p = parse("serde<2.0.0").unwrap();
        let yes = vec![("serde".into(), v("1.9.0"))];
        assert!(p.matches(&yes));
        let no = vec![("serde".into(), v("2.0.0"))];
        assert!(!p.matches(&no));
    }

    #[test]
    fn matches_version_lte() {
        let p = parse("serde<=1.5.0").unwrap();
        let exact = vec![("serde".into(), v("1.5.0"))];
        assert!(p.matches(&exact));
        let below = vec![("serde".into(), v("1.4.0"))];
        assert!(p.matches(&below));
        let above = vec![("serde".into(), v("1.5.1"))];
        assert!(!p.matches(&above));
    }

    #[test]
    fn matches_version_tilde() {
        // ~1.2 matches >=1.2.0, <1.3.0
        let p = parse("serde~1.2").unwrap();
        let yes = vec![("serde".into(), v("1.2.5"))];
        assert!(p.matches(&yes));
        let no = vec![("serde".into(), v("1.3.0"))];
        assert!(!p.matches(&no));
    }

    // --- Display (round-trip) ---

    #[test]
    fn display_roundtrip() {
        let cases = ["serde", "serde>=1.0"];
        for input in cases {
            let p = parse(input).unwrap();
            let displayed = p.to_string();
            let reparsed = parse(&displayed).unwrap();
            assert_eq!(p, reparsed, "round-trip failed for: {input}");
        }
    }

    // --- Evaluator tests ---

    #[test]
    fn matches_bare_name_present() {
        let p = parse("serde").unwrap();
        assert!(p.matches(&workspace()));
    }

    #[test]
    fn matches_bare_name_absent() {
        let p = parse("reqwest").unwrap();
        assert!(!p.matches(&workspace()));
    }

    #[test]
    fn matches_version_satisfied() {
        let p = parse("serde>=1.0").unwrap();
        assert!(p.matches(&workspace()));
    }

    #[test]
    fn matches_version_not_satisfied() {
        let p = parse("serde>=2.0").unwrap();
        assert!(!p.matches(&workspace()));
    }

    // --- references_crate tests ---

    #[test]
    fn references_crate_bare() {
        let p = parse("serde").unwrap();
        assert!(p.references_crate("serde"));
        assert!(!p.references_crate("tokio"));
    }

    // --- parse_comma_separated tests ---

    #[test]
    fn comma_separated_single() {
        let preds = parse_comma_separated("serde").unwrap();
        assert_eq!(preds.len(), 1);
        assert_eq!(preds[0].name, "serde");
    }

    #[test]
    fn comma_separated_multiple() {
        let preds = parse_comma_separated("serde, tokio>=1.0, anyhow").unwrap();
        assert_eq!(preds.len(), 3);
        assert_eq!(preds[0].name, "serde");
        assert_eq!(preds[1].name, "tokio");
        assert!(preds[1].version_req.is_some());
        assert_eq!(preds[2].name, "anyhow");
    }

    #[test]
    fn comma_separated_empty() {
        let preds = parse_comma_separated("").unwrap();
        assert!(preds.is_empty());
    }

    #[test]
    fn comma_separated_whitespace() {
        let preds = parse_comma_separated("  serde  ,  tokio  ").unwrap();
        assert_eq!(preds.len(), 2);
    }

    // --- Error tests ---

    #[test]
    fn parse_error_empty() {
        assert!(parse("").is_err());
    }

    #[test]
    fn parse_error_trailing() {
        assert!(parse("serde blah").is_err());
    }
}
