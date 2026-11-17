//! Reading a `SKILL.md` as Symposium does: the YAML between the opening `---` and the
//! next line starting `---`.
//!
//! This reader is the checker's own; only the predicate syntax comes from
//! `symposium-sdk`, which defines it.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_yaml_ng::Value;
use symposium_sdk::predicate::{DependsOnList, PredicateSet};
use thiserror::Error;

/// Larger than any `SKILL.md` a skill is identified by.
pub(crate) const MAX_SKILL_MD: u64 = 8 * 1024 * 1024;

/// The longest `description`, in bytes, as Symposium measures it.
const MAX_DESCRIPTION: usize = 1024;

/// Why Symposium would not load a registry plugin's skill.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum Unloadable {
    #[error("it has no `---` frontmatter")]
    NoFrontmatter,
    /// Not a mapping with string keys, or a field Symposium reads is not a string.
    #[error("its frontmatter is malformed: {0}")]
    Malformed(String),
    #[error("its frontmatter uses the retired `crates` field")]
    Crates,
    #[error("its frontmatter has no `name`")]
    NoName,
    #[error("its frontmatter `name` {0:?} is not a plain directory name")]
    UnsafeName(String),
    #[error("its frontmatter has no `description`")]
    NoDescription,
    #[error("its frontmatter `description` is empty")]
    EmptyDescription,
    #[error("its frontmatter `description` is longer than {MAX_DESCRIPTION} bytes")]
    LongDescription,
    #[error("its frontmatter `{field}` does not parse: {message}")]
    Predicates {
        field: &'static str,
        message: String,
    },
}

/// The frontmatter fields Symposium reads, each a string when present. Any other field
/// may hold anything, but every key must be a string.
#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(rename = "depends-on")]
    depends_on: Option<String>,
    predicates: Option<String>,
    #[serde(flatten)]
    other: BTreeMap<String, IgnoredAny>,
}

impl Frontmatter {
    /// The skill's name, if Symposium would load it: named safely, described, and gated
    /// by predicates that parse.
    fn loadable_name(self) -> Result<String, Unloadable> {
        if self.other.contains_key("crates") {
            return Err(Unloadable::Crates);
        }
        let name = unquoted(self.name.as_deref().ok_or(Unloadable::NoName)?);
        if !is_safe_dir_name(name) {
            return Err(Unloadable::UnsafeName(name.to_owned()));
        }
        let description = self
            .description
            .as_deref()
            .ok_or(Unloadable::NoDescription)?;
        match description.trim().len() {
            0 => return Err(Unloadable::EmptyDescription),
            length if length > MAX_DESCRIPTION => return Err(Unloadable::LongDescription),
            _ => {}
        }
        parses(
            "depends-on",
            self.depends_on.as_deref(),
            DependsOnList::parse,
        )?;
        parses(
            "predicates",
            self.predicates.as_deref(),
            PredicateSet::parse,
        )?;
        Ok(name.to_owned())
    }
}

/// Whether `field`, when present, parses with `parse`.
fn parses<T>(
    field: &'static str,
    value: Option<&str>,
    parse: impl FnOnce(&str) -> anyhow::Result<T>,
) -> Result<(), Unloadable> {
    let Some(value) = value else {
        return Ok(());
    };
    parse(value)
        .map(drop)
        .map_err(|error| Unloadable::Predicates {
            field,
            message: format!("{error:#}"),
        })
}

/// The YAML of a `SKILL.md`'s frontmatter.
fn frontmatter(text: &str) -> Option<&str> {
    let body = text.trim_start().strip_prefix("---")?;
    let body = body.strip_prefix('\n').unwrap_or(body);
    Some(&body[..body.find("\n---")?])
}

/// Surrounding double quotes removed, as Symposium removes them from a name.
fn unquoted(name: &str) -> &str {
    name.strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(name)
}

/// The `name` of a `SKILL.md`, read as Symposium reads it, or `None` when it has none.
pub(crate) fn name(text: &str) -> Option<String> {
    let frontmatter: Value = serde_yaml_ng::from_str(frontmatter(text)?).ok()?;
    let name = frontmatter.get("name")?.as_str()?;
    Some(unquoted(name).to_owned())
}

/// The name of a registry plugin's skill, if Symposium would load it.
pub(crate) fn loadable_name(text: &str) -> Result<String, Unloadable> {
    let yaml = frontmatter(text).ok_or(Unloadable::NoFrontmatter)?;
    let malformed = |error: serde_yaml_ng::Error| Unloadable::Malformed(error.to_string());
    let value: Value = serde_yaml_ng::from_str(yaml).map_err(malformed)?;
    // Read as a struct, a number key would become a string; Symposium refuses it.
    if let Some(mapping) = value.as_mapping()
        && mapping.keys().any(|key| !key.is_string())
    {
        return Err(Unloadable::Malformed("a key is not a string".to_owned()));
    }
    serde_yaml_ng::from_value::<Frontmatter>(value)
        .map_err(malformed)?
        .loadable_name()
}

/// A name that cannot resolve outside the skills directory, as Symposium requires:
/// Windows trims trailing dots and spaces, so a name of only those is the parent.
fn is_safe_dir_name(name: &str) -> bool {
    let only_dots_and_spaces = name.chars().all(|c| c == '.' || c == ' ');
    !only_dots_and_spaces && !name.contains(['/', '\\', ':'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frontmatter_name_is_read_as_yaml() {
        for text in [
            "---\nname: x\n---\n",
            "---\nname: \"x\"\n---\n",
            "---\nname: 'x'\n---\n",
            "---\nname: x # explanatory comment\n---\n",
            "---\ndescription: |\n  two\n  lines\nname: >-\n  x\n---\n",
            "\n---\r\nname: x\r\n---\r\n",
        ] {
            assert_eq!(name(text).as_deref(), Some("x"), "{text:?}");
        }
    }

    fn loadable(frontmatter: &str) -> Result<String, Unloadable> {
        loadable_name(&format!("---\n{frontmatter}\n---\nBody.\n"))
    }

    #[test]
    fn a_skill_symposium_loads_is_named() {
        let named = loadable(
            "name: \"my-skill\"\ndescription: Use it.\ndepends-on: serde, tokio>=1\nmetadata: [kept]",
        );

        assert_eq!(named.as_deref(), Ok("my-skill"));
    }

    #[test]
    fn a_skill_symposium_would_not_load_is_refused() {
        let long = "x".repeat(MAX_DESCRIPTION + 1);
        let cases = [
            ("description: d", Unloadable::NoName),
            (
                "name: ..\ndescription: d",
                Unloadable::UnsafeName("..".to_owned()),
            ),
            (
                "name: a/b\ndescription: d",
                Unloadable::UnsafeName("a/b".to_owned()),
            ),
            ("name: a", Unloadable::NoDescription),
            ("name: a\ndescription: \"  \"", Unloadable::EmptyDescription),
            (
                &format!("name: a\ndescription: {long}"),
                Unloadable::LongDescription,
            ),
            ("name: a\ndescription: d\ncrates: serde", Unloadable::Crates),
        ];
        for (frontmatter, expected) in cases {
            assert_eq!(loadable(frontmatter), Err(expected), "{frontmatter}");
        }
        let malformed = [
            "name: [a]\ndescription: d",
            "name: a\ndescription: d\ndepends-on: [serde]",
            "- a\n- b",
            "1: a\nname: a\ndescription: d",
        ];
        for frontmatter in malformed {
            let result = loadable(frontmatter);
            assert!(
                matches!(result, Err(Unloadable::Malformed(_))),
                "{frontmatter}: {result:?}"
            );
        }
        assert_eq!(
            loadable_name("# No frontmatter\n"),
            Err(Unloadable::NoFrontmatter)
        );
    }

    #[test]
    fn a_skill_whose_predicates_do_not_parse_is_refused() {
        for (field, value) in [("depends-on", "serde>>1"), ("predicates", "nonsense(")] {
            let result = loadable(&format!("name: a\ndescription: d\n{field}: \"{value}\""));

            assert!(
                matches!(&result, Err(Unloadable::Predicates { field: found, .. }) if *found == field),
                "{result:?}"
            );
        }
    }

    #[test]
    fn a_frontmatter_without_a_name_names_nothing() {
        for text in [
            "name: x\n",
            "---\nname: x\n",
            "---\nname: [x\n---\n",
            "---\nname: [x]\n---\n",
            "---\ndescription: d\n---\n",
        ] {
            assert_eq!(name(text), None, "{text:?}");
        }
    }
}
