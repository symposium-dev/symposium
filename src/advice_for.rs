//! `advice-for` predicate language for matching crate dependencies.
//!
//! Skills declare which crates they advise on using a mini predicate language:
//!
//! ```text
//! serde                          -- bare crate name (any version)
//! serde>=1.0                     -- crate with version constraint
//! any(serde, serde_json)         -- matches if any child matches
//! all(tokio, tokio-stream>=0.1)  -- matches if all children match
//! any(all(axum, tower), actix)   -- nesting allowed
//! ```

use anyhow::{Context, Result, bail};

/// A predicate over workspace crate dependencies.
///
/// Serializes to/from its string representation (e.g., `"serde>=1.0"`,
/// `"any(serde, tokio)"`).
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Matches a single crate, optionally constrained by version.
    Crate {
        name: String,
        version_req: Option<semver::VersionReq>,
    },
    /// Matches if any child predicate matches.
    Any(Vec<Predicate>),
    /// Matches if all child predicates match.
    All(Vec<Predicate>),
}

impl Predicate {
    /// Evaluate this predicate against a workspace dependency list.
    pub fn matches(&self, deps: &[(String, semver::Version)]) -> bool {
        match self {
            Predicate::Crate { name, version_req } => deps.iter().any(|(dep_name, dep_ver)| {
                dep_name == name
                    && version_req
                        .as_ref()
                        .map_or(true, |req| req.matches(dep_ver))
            }),
            Predicate::Any(children) => children.iter().any(|c| c.matches(deps)),
            Predicate::All(children) => children.iter().all(|c| c.matches(deps)),
        }
    }

    /// Returns true if this predicate references the given crate name anywhere
    /// in its tree. Used to filter skills to those relevant to a specific crate
    /// query (e.g., `symposium crate serde` only shows serde-related skills).
    pub fn references_crate(&self, name: &str) -> bool {
        match self {
            Predicate::Crate {
                name: crate_name, ..
            } => crate_name == name,
            Predicate::Any(children) | Predicate::All(children) => {
                children.iter().any(|c| c.references_crate(name))
            }
        }
    }

    /// Collect all crate names referenced by this predicate into a set.
    pub fn collect_crate_names(&self, out: &mut std::collections::BTreeSet<String>) {
        match self {
            Predicate::Crate { name, .. } => {
                out.insert(name.clone());
            }
            Predicate::Any(children) | Predicate::All(children) => {
                for child in children {
                    child.collect_crate_names(out);
                }
            }
        }
    }
}

/// Parse a list of predicate strings.
///
/// Each string can be a bare crate name, a crate with version constraint,
/// or a combinator like `any(...)` / `all(...)`.
pub fn parse_predicates(strings: &[String]) -> Result<Vec<Predicate>> {
    strings
        .iter()
        .map(|s| parse(s).with_context(|| format!("failed to parse predicate: {s:?}")))
        .collect()
}

/// Parse an `advice-for` predicate string.
fn parse(input: &str) -> Result<Predicate> {
    let mut parser = Parser::new(input);
    let pred = parser.parse_predicate()?;
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

    fn parse_predicate(&mut self) -> Result<Predicate> {
        self.skip_whitespace();
        if self.remaining().starts_with("any(") {
            self.parse_combinator("any").map(Predicate::Any)
        } else if self.remaining().starts_with("all(") {
            self.parse_combinator("all").map(Predicate::All)
        } else {
            self.parse_atom()
        }
    }

    fn parse_combinator(&mut self, keyword: &str) -> Result<Vec<Predicate>> {
        self.pos += keyword.len() + 1; // skip "any(" or "all("
        self.skip_whitespace();

        if self.pos >= self.input.len() {
            bail!("unexpected end of input after `{keyword}(`");
        }

        // Handle empty combinator: any() or all()
        if self.remaining().starts_with(')') {
            bail!("`{keyword}()` requires at least one argument");
        }

        let mut items = vec![self.parse_predicate()?];
        loop {
            self.skip_whitespace();
            match self.input.as_bytes().get(self.pos) {
                Some(b',') => {
                    self.pos += 1;
                    items.push(self.parse_predicate()?);
                }
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    bail!(
                        "expected ',' or ')' at position {}: {:?}",
                        self.pos,
                        self.remaining()
                    );
                }
                None => bail!("unexpected end of input, expected ')' for `{keyword}`"),
            }
        }
        Ok(items)
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
                // Consume until delimiter: ',', ')', whitespace, or end
                while self.pos < self.input.len() {
                    let c = self.input.as_bytes()[self.pos];
                    if matches!(c, b',' | b')') || c.is_ascii_whitespace() {
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

        Ok(Predicate::Crate {
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
        match self {
            Predicate::Crate { name, version_req } => {
                write!(f, "{name}")?;
                if let Some(req) = version_req {
                    write!(f, "{req}")?;
                }
                Ok(())
            }
            Predicate::Any(children) => {
                write!(f, "any(")?;
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{child}")?;
                }
                write!(f, ")")
            }
            Predicate::All(children) => {
                write!(f, "all(")?;
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{child}")?;
                }
                write!(f, ")")
            }
        }
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
            Predicate::Crate {
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
            Predicate::Crate {
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
            Predicate::Crate {
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
            Predicate::Crate {
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
            Predicate::Crate {
                name: "serde".into(),
                version_req: Some(semver::VersionReq::parse("^1.2").unwrap()),
            }
        );
    }

    #[test]
    fn parse_version_gte_not_rewritten() {
        // `>=1.0` stays as `>=1.0`, not rewritten to `^`
        let p = parse("serde>=1.0").unwrap();
        let Predicate::Crate { version_req, .. } = &p else {
            panic!("expected Crate");
        };
        let req = version_req.as_ref().unwrap();
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
            Predicate::Crate {
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

    #[test]
    fn parse_any() {
        let p = parse("any(serde, serde_json)").unwrap();
        assert_eq!(
            p,
            Predicate::Any(vec![
                Predicate::Crate {
                    name: "serde".into(),
                    version_req: None,
                },
                Predicate::Crate {
                    name: "serde_json".into(),
                    version_req: None,
                },
            ])
        );
    }

    #[test]
    fn parse_all_with_version() {
        let p = parse("all(tokio, tokio-stream>=0.1.10)").unwrap();
        assert_eq!(
            p,
            Predicate::All(vec![
                Predicate::Crate {
                    name: "tokio".into(),
                    version_req: None,
                },
                Predicate::Crate {
                    name: "tokio-stream".into(),
                    version_req: Some(semver::VersionReq::parse(">=0.1.10").unwrap()),
                },
            ])
        );
    }

    #[test]
    fn parse_nested() {
        let p = parse("any(all(axum, tower), all(actix-web, actix-rt))").unwrap();
        assert_eq!(
            p,
            Predicate::Any(vec![
                Predicate::All(vec![
                    Predicate::Crate {
                        name: "axum".into(),
                        version_req: None,
                    },
                    Predicate::Crate {
                        name: "tower".into(),
                        version_req: None,
                    },
                ]),
                Predicate::All(vec![
                    Predicate::Crate {
                        name: "actix-web".into(),
                        version_req: None,
                    },
                    Predicate::Crate {
                        name: "actix-rt".into(),
                        version_req: None,
                    },
                ]),
            ])
        );
    }

    #[test]
    fn parse_with_whitespace() {
        let p = parse("  any( serde , serde_json )  ").unwrap();
        assert_eq!(
            p,
            Predicate::Any(vec![
                Predicate::Crate {
                    name: "serde".into(),
                    version_req: None,
                },
                Predicate::Crate {
                    name: "serde_json".into(),
                    version_req: None,
                },
            ])
        );
    }

    #[test]
    fn parse_error_empty() {
        assert!(parse("").is_err());
    }

    #[test]
    fn parse_error_empty_any() {
        assert!(parse("any()").is_err());
    }

    #[test]
    fn parse_error_unclosed() {
        assert!(parse("any(serde, serde_json").is_err());
    }

    #[test]
    fn parse_error_trailing() {
        assert!(parse("serde blah").is_err());
    }

    // --- Display (round-trip) ---

    #[test]
    fn display_roundtrip() {
        let cases = [
            "serde",
            "any(serde, serde_json)",
            "all(tokio, tokio-stream>=0.1.10)",
            "any(all(axum, tower), all(actix-web, actix-rt))",
        ];
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

    #[test]
    fn matches_any_one_present() {
        let p = parse("any(serde, reqwest)").unwrap();
        assert!(p.matches(&workspace()));
    }

    #[test]
    fn matches_any_none_present() {
        let p = parse("any(reqwest, hyper)").unwrap();
        assert!(!p.matches(&workspace()));
    }

    #[test]
    fn matches_all_both_present() {
        let p = parse("all(serde, tokio)").unwrap();
        assert!(p.matches(&workspace()));
    }

    #[test]
    fn matches_all_one_missing() {
        let p = parse("all(serde, reqwest)").unwrap();
        assert!(!p.matches(&workspace()));
    }

    // --- references_crate tests ---

    #[test]
    fn references_crate_bare() {
        let p = parse("serde").unwrap();
        assert!(p.references_crate("serde"));
        assert!(!p.references_crate("tokio"));
    }

    #[test]
    fn references_crate_nested() {
        let p = parse("any(all(serde, serde_json), tokio)").unwrap();
        assert!(p.references_crate("serde"));
        assert!(p.references_crate("serde_json"));
        assert!(p.references_crate("tokio"));
        assert!(!p.references_crate("anyhow"));
    }
}
