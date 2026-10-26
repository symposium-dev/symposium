//! Hidden tests in `checks/`: one file per test, exactly one test per file.
//!
//! The rule must hold before any starter or solution compiles against a check, so it is
//! proved on the source with `syn` rather than by listing tests at runtime. The accepted
//! syntax is closed: anything that could add, remove, or hide a test is rejected rather
//! than interpreted. That covers `cfg` and `cfg_attr` anywhere, attribute and item
//! macros, out-of-line modules, and tests inside modules. Grading still confirms the
//! count with libtest's `--list`.

use std::fs;
use std::io;
use std::path::Path;

use syn::{Attribute, Item, ItemFn, Meta, Signature};
use thiserror::Error;

use super::{CheckName, DirProblem, NameError};

#[derive(Debug, Error)]
pub(crate) enum CheckError {
    #[error("`checks/` {0}")]
    Dir(DirProblem),
    #[error("cannot inspect `checks/`")]
    Inspect(#[source] io::Error),
    #[error("`checks/` holds no hidden tests")]
    Empty,
    #[error("cannot inspect `checks/{file}`")]
    Metadata {
        file: String,
        #[source]
        source: io::Error,
    },
    #[error("`checks/{0}` is not a regular `.rs` file")]
    NotRustFile(String),
    #[error("`checks/{file}` has an unusable name")]
    Name {
        file: String,
        #[source]
        source: NameError,
    },
    #[error(
        "`checks/{0}` must be named like a Rust identifier (letters, digits, `_`, `-`), since Cargo builds it as a test target of that name"
    )]
    TargetName(String),
    #[error("cannot read `checks/{file}`")]
    Read {
        file: String,
        #[source]
        source: io::Error,
    },
    #[error("`checks/{file}` is not valid Rust at line {line}, column {column}")]
    Syntax {
        file: String,
        line: usize,
        column: usize,
        #[source]
        source: syn::Error,
    },
    #[error("`checks/{file}` holds {count} top-level tests; it must hold exactly one")]
    TestCount { file: String, count: usize },
    #[error("`checks/{file}` uses `#[{attribute}]`, which can add or remove tests")]
    Conditional { file: String, attribute: String },
    #[error(
        "`checks/{file}` uses `#[{attribute}]`; only built-in attributes that cannot change the tests are allowed"
    )]
    UnsupportedAttribute { file: String, attribute: String },
    #[error("`checks/{file}` marks its test `#[ignore]`, so it would not run")]
    Ignored { file: String },
    #[error("`checks/{file}` passes arguments to `#[test]`")]
    TestArguments { file: String },
    #[error(
        "`checks/{file}` declares test `{function}` with parameters, generics, `async`, or `unsafe`"
    )]
    TestSignature { file: String, function: String },
    #[error("`checks/{file}` has a test inside a module")]
    NestedTest { file: String },
    #[error("`checks/{file}` declares `mod {module};`; a check must be one self-contained file")]
    ExternalModule { file: String, module: String },
    #[error("`checks/{file}` invokes a macro at item level, which could expand to tests")]
    ItemMacro { file: String },
    #[error("`checks/{file}` holds an item `syn` cannot interpret")]
    UnsupportedItem { file: String },
}

/// Validates every file in `dir` and returns the check names, sorted.
pub(super) fn load(dir: &Path) -> Result<Vec<CheckName>, Vec<CheckError>> {
    let entries = sorted_entries(dir).map_err(|error| vec![error])?;
    let mut errors = Vec::new();
    let names: Vec<CheckName> = entries
        .iter()
        .filter_map(|entry| check_file(entry, &mut errors))
        .collect();
    if names.is_empty() && errors.is_empty() {
        errors.push(CheckError::Empty);
    }
    if errors.is_empty() {
        Ok(names)
    } else {
        Err(errors)
    }
}

/// The entries of `dir`, sorted by name, once it is shown to be a real directory.
fn sorted_entries(dir: &Path) -> Result<Vec<fs::DirEntry>, CheckError> {
    match DirProblem::of(dir) {
        Ok(None) => {}
        Ok(Some(problem)) => return Err(CheckError::Dir(problem)),
        Err(source) => return Err(CheckError::Inspect(source)),
    }
    let mut entries = fs::read_dir(dir)
        .and_then(Iterator::collect::<io::Result<Vec<_>>>)
        .map_err(CheckError::Inspect)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

/// Validates one entry of `checks/`, pushing its problems. Returns its check name when
/// the entry is a `.rs` file with a usable name, even if its source is rejected.
fn check_file(entry: &fs::DirEntry, errors: &mut Vec<CheckError>) -> Option<CheckName> {
    let file_name = entry.file_name();
    let file = file_name.to_string_lossy().into_owned();
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(source) => {
            errors.push(CheckError::Metadata { file, source });
            return None;
        }
    };
    let stem = file_name
        .to_str()
        .and_then(|name| name.strip_suffix(".rs"))
        .filter(|_| file_type.is_file());
    let Some(stem) = stem else {
        errors.push(CheckError::NotRustFile(file));
        return None;
    };
    let name = match CheckName::new(stem) {
        Ok(name) => name,
        Err(source) => {
            errors.push(CheckError::Name { file, source });
            return None;
        }
    };
    if !is_target_name(stem) {
        errors.push(CheckError::TargetName(file));
        return Some(name);
    }
    match fs::read_to_string(entry.path()) {
        Ok(source) => errors.extend(check_source(&file, &source)),
        Err(source) => errors.push(CheckError::Read { file, source }),
    }
    Some(name)
}

fn check_source(file: &str, source: &str) -> Vec<CheckError> {
    let parsed = match parse(file, source) {
        Ok(parsed) => parsed,
        Err(error) => return vec![error],
    };
    let mut scan = Scan {
        file,
        tests: 0,
        disguised_test: false,
        errors: Vec::new(),
    };
    scan.inert_attributes(&parsed.attrs);
    scan.items(&parsed.items, true);
    if scan.tests != 1 && !scan.disguised_test {
        scan.errors.push(CheckError::TestCount {
            file: file.to_owned(),
            count: scan.tests,
        });
    }
    scan.errors
}

fn parse(file: &str, source: &str) -> Result<syn::File, CheckError> {
    syn::parse_file(source).map_err(|source| {
        let start = source.span().start();
        CheckError::Syntax {
            file: file.to_owned(),
            line: start.line,
            // `proc_macro2` counts columns from zero; editors count from one.
            column: start.column + 1,
            source,
        }
    })
}

/// What one attribute can do to the set of tests.
enum AttributeKind {
    Test,
    TestWithArguments,
    Ignore,
    ShouldPanic,
    /// `cfg` or `cfg_attr`.
    Conditional(String),
    /// Built-in and unable to add, remove, or hide a test.
    Inert,
    Unsupported(String),
}

/// Built-in attributes that cannot change which tests exist.
const INERT_ATTRIBUTES: [&str; 13] = [
    "allow",
    "cold",
    "deny",
    "deprecated",
    "doc",
    "expect",
    "forbid",
    "inline",
    "must_use",
    "non_exhaustive",
    "repr",
    "track_caller",
    "warn",
];

/// Derives the standard library provides; a derive macro from elsewhere could emit tests.
const STD_DERIVES: [&str; 9] = [
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Eq",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
];

/// What a function's attributes say about it as a test.
#[derive(Default)]
struct TestMarkers {
    is_test: bool,
    ignored: bool,
}

struct Scan<'a> {
    file: &'a str,
    tests: usize,
    /// A function carries a rejected attribute, such as `#[tokio::test]`, so it may be a
    /// test the scan cannot recognize and the count would mislead.
    disguised_test: bool,
    errors: Vec<CheckError>,
}

impl Scan<'_> {
    fn items(&mut self, items: &[Item], top_level: bool) {
        for item in items {
            match item {
                Item::Fn(function) => self.function(function, top_level),
                Item::Mod(module) => {
                    self.inert_attributes(&module.attrs);
                    match &module.content {
                        Some((_, inner)) => self.items(inner, false),
                        None => self.errors.push(CheckError::ExternalModule {
                            file: self.file.to_owned(),
                            module: module.ident.to_string(),
                        }),
                    }
                }
                Item::Macro(_) => self.errors.push(CheckError::ItemMacro {
                    file: self.file.to_owned(),
                }),
                other => match item_attributes(other) {
                    Some(attributes) => self.inert_attributes(attributes),
                    None => self.errors.push(CheckError::UnsupportedItem {
                        file: self.file.to_owned(),
                    }),
                },
            }
        }
    }

    fn function(&mut self, function: &ItemFn, top_level: bool) {
        let TestMarkers { is_test, ignored } = self.test_markers(&function.attrs);
        if !is_test {
            return;
        }
        if !top_level {
            self.errors.push(CheckError::NestedTest {
                file: self.file.to_owned(),
            });
            return;
        }
        self.tests += 1;
        if ignored {
            self.errors.push(CheckError::Ignored {
                file: self.file.to_owned(),
            });
        }
        self.test_signature(&function.sig);
    }

    /// Reads a function's attributes, rejecting any that could change the tests.
    fn test_markers(&mut self, attributes: &[Attribute]) -> TestMarkers {
        let mut markers = TestMarkers::default();
        for attribute in attributes {
            match classify(attribute) {
                AttributeKind::Test => markers.is_test = true,
                AttributeKind::TestWithArguments => {
                    markers.is_test = true;
                    self.errors.push(CheckError::TestArguments {
                        file: self.file.to_owned(),
                    });
                }
                AttributeKind::Ignore => markers.ignored = true,
                AttributeKind::ShouldPanic | AttributeKind::Inert => {}
                kind => {
                    self.disguised_test = true;
                    self.reject(kind);
                }
            }
        }
        markers
    }

    /// A test must be a plain `fn name()`, which libtest can call directly.
    fn test_signature(&mut self, signature: &Signature) {
        let generics = &signature.generics;
        if signature.asyncness.is_some()
            || signature.unsafety.is_some()
            || !signature.inputs.is_empty()
            || signature.variadic.is_some()
            || !generics.params.is_empty()
            || generics.where_clause.is_some()
        {
            self.errors.push(CheckError::TestSignature {
                file: self.file.to_owned(),
                function: signature.ident.to_string(),
            });
        }
    }

    /// Attributes on anything but a function, and file-level attributes: only inert
    /// ones are allowed.
    fn inert_attributes(&mut self, attributes: &[Attribute]) {
        for attribute in attributes {
            match classify(attribute) {
                AttributeKind::Inert => {}
                AttributeKind::Test
                | AttributeKind::TestWithArguments
                | AttributeKind::Ignore
                | AttributeKind::ShouldPanic => {
                    self.reject(AttributeKind::Unsupported(path_text(attribute)));
                }
                kind => self.reject(kind),
            }
        }
    }

    fn reject(&mut self, kind: AttributeKind) {
        let file = self.file.to_owned();
        match kind {
            AttributeKind::Conditional(attribute) => {
                self.errors
                    .push(CheckError::Conditional { file, attribute });
            }
            AttributeKind::Unsupported(attribute) => {
                self.errors
                    .push(CheckError::UnsupportedAttribute { file, attribute });
            }
            AttributeKind::Test
            | AttributeKind::TestWithArguments
            | AttributeKind::Ignore
            | AttributeKind::ShouldPanic
            | AttributeKind::Inert => {}
        }
    }
}

fn classify(attribute: &Attribute) -> AttributeKind {
    let path = attribute.path();
    let Some(ident) = path.get_ident().map(ToString::to_string) else {
        return AttributeKind::Unsupported(path_text(attribute));
    };
    match ident.as_str() {
        "test" if matches!(attribute.meta, Meta::Path(_)) => AttributeKind::Test,
        "test" => AttributeKind::TestWithArguments,
        "ignore" => AttributeKind::Ignore,
        "should_panic" => AttributeKind::ShouldPanic,
        "cfg" | "cfg_attr" => AttributeKind::Conditional(ident),
        "derive" => match foreign_derive(attribute) {
            None => AttributeKind::Inert,
            Some(derive) => AttributeKind::Unsupported(format!("derive({derive})")),
        },
        name if INERT_ATTRIBUTES.contains(&name) => AttributeKind::Inert,
        _ => AttributeKind::Unsupported(ident),
    }
}

/// The first derive that is not from the standard library, or `..` when the list cannot
/// be read.
fn foreign_derive(attribute: &Attribute) -> Option<String> {
    let mut foreign = None;
    let parsed = attribute.parse_nested_meta(|meta| {
        let is_std = meta
            .path
            .get_ident()
            .is_some_and(|ident| STD_DERIVES.iter().any(|derive| ident == derive));
        if !is_std && foreign.is_none() {
            foreign = Some(
                meta.path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::"),
            );
        }
        Ok(())
    });
    match parsed {
        Ok(()) => foreign,
        Err(_) => Some("..".to_owned()),
    }
}

/// `None` for item kinds `syn` could not interpret.
fn item_attributes(item: &Item) -> Option<&[Attribute]> {
    let attributes = match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => return None,
    };
    Some(attributes)
}

fn path_text(attribute: &Attribute) -> String {
    attribute
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// Cargo names a test target after its file, and the target is compiled as a crate, so
/// the name must work as a crate name: an identifier, where `-` stands for `_`.
fn is_target_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|rest| rest.is_ascii_alphanumeric() || rest == '_' || rest == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(source: &str) -> Vec<String> {
        check_source("demo.rs", source)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn check_names_must_work_as_cargo_targets() {
        for name in [
            "works",
            "settings_survive_reopen",
            "store-durability",
            "_private",
            "v2",
        ] {
            assert!(is_target_name(name), "{name}");
        }
        for name in ["a.b", "2fast", "-dash", "café", ""] {
            assert!(!is_target_name(name), "{name}");
        }
    }

    #[test]
    fn one_plain_test_with_inert_items_passes() {
        let source = r#"
            #![allow(dead_code)]
            use demo::double;

            #[derive(Debug, Clone, PartialEq)]
            struct Pair(i32, i32);

            /// Checks doubling.
            #[test]
            #[should_panic(expected = "boom")]
            fn doubles() -> Result<(), String> { double(1); Ok(()) }

            mod helpers { pub fn helper() {} }
        "#;

        assert_eq!(messages(source), Vec::<String>::new());
    }

    #[test]
    fn test_count_must_be_exactly_one() {
        assert_eq!(
            messages(""),
            ["`checks/demo.rs` holds 0 top-level tests; it must hold exactly one"]
        );
        assert_eq!(
            messages("#[test] fn first() {}\n#[test] fn second() {}\n"),
            ["`checks/demo.rs` holds 2 top-level tests; it must hold exactly one"]
        );
    }

    #[test]
    fn a_disguised_test_is_not_also_miscounted() {
        assert_eq!(
            messages("#[tokio::test] async fn works() {}"),
            [
                "`checks/demo.rs` uses `#[tokio::test]`; only built-in attributes that cannot change the tests are allowed"
            ]
        );
    }

    #[test]
    fn conditional_compilation_is_rejected_anywhere() {
        let cases = [
            ("#![cfg(windows)]\n#[test] fn works() {}", "cfg"),
            (
                "#[cfg_attr(windows, test)] fn extra() {}\n#[test] fn works() {}",
                "cfg_attr",
            ),
            ("#[test] #[cfg(windows)] fn works() {}", "cfg"),
            ("#[cfg(test)] struct Hidden;\n#[test] fn works() {}", "cfg"),
        ];
        for (source, attribute) in cases {
            assert_eq!(
                messages(source),
                [format!(
                    "`checks/demo.rs` uses `#[{attribute}]`, which can add or remove tests"
                )],
                "{source}"
            );
        }
    }

    #[test]
    fn constructs_that_can_hide_tests_are_rejected() {
        const UNSUPPORTED: &str =
            "only built-in attributes that cannot change the tests are allowed";
        let cases = [
            (
                "mod more_tests;\n#[test] fn works() {}",
                "`checks/demo.rs` declares `mod more_tests;`; a check must be one self-contained file".to_owned(),
            ),
            (
                "#[path = \"x.rs\"] mod more {}\n#[test] fn works() {}",
                format!("`checks/demo.rs` uses `#[path]`; {UNSUPPORTED}"),
            ),
            (
                "make_tests!();\n#[test] fn works() {}",
                "`checks/demo.rs` invokes a macro at item level, which could expand to tests".to_owned(),
            ),
            (
                "mod inner { make_tests!(); }\n#[test] fn works() {}",
                "`checks/demo.rs` invokes a macro at item level, which could expand to tests".to_owned(),
            ),
            (
                "mod inner { #[test] fn hidden() {} }\n#[test] fn works() {}",
                "`checks/demo.rs` has a test inside a module".to_owned(),
            ),
            (
                "#[tokio::test] async fn runs() {}\n#[test] fn works() {}",
                format!("`checks/demo.rs` uses `#[tokio::test]`; {UNSUPPORTED}"),
            ),
            (
                "#[derive(Debug, MakeTests)] struct Pair;\n#[test] fn works() {}",
                format!("`checks/demo.rs` uses `#[derive(MakeTests)]`; {UNSUPPORTED}"),
            ),
        ];
        for (source, message) in cases {
            assert_eq!(messages(source), [message], "{source}");
        }
    }

    #[test]
    fn tests_that_libtest_would_not_run_as_written_are_rejected() {
        assert_eq!(
            messages("#[test] #[ignore] fn skipped() {}"),
            ["`checks/demo.rs` marks its test `#[ignore]`, so it would not run"]
        );
        assert_eq!(
            messages("#[test(anything)] fn works() {}"),
            ["`checks/demo.rs` passes arguments to `#[test]`"]
        );
        for signature in [
            "async fn works()",
            "unsafe fn works()",
            "fn works(x: i32)",
            "fn works<T>()",
        ] {
            assert_eq!(
                messages(&format!("#[test] {signature} {{}}")),
                [
                    "`checks/demo.rs` declares test `works` with parameters, generics, `async`, or `unsafe`"
                ],
                "{signature}"
            );
        }
    }

    #[test]
    fn invalid_rust_reports_where_it_fails() {
        let errors = check_source("demo.rs", "#[test]\nfn works() {}\nfn broken() -> {}\n");

        assert!(
            matches!(
                errors.as_slice(),
                [CheckError::Syntax {
                    line: 3,
                    column: 16,
                    ..
                }]
            ),
            "{errors:?}"
        );
    }
}
