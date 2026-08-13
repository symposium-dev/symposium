//! Snapshots of generated declarations for real MCP servers.
//!
//! The payloads in `testdata/` are verbatim `tools/list` responses captured
//! from published servers, chosen to span four independent schema generators:
//! two flavours of the TypeScript `zod` toolchain (draft-07 and 2020-12),
//! plus the reference servers' own hand-written schemas.
//!
//! These are the tests that catch a dialect we handle wrongly. A construct
//! that starts rendering as `unknown` shows up as a diff in the checked-in
//! `.d.ts`, which the synthetic unit tests cannot detect because they only
//! cover shapes we already thought of.
//!
//! Regenerate with `UPDATE_EXPECT=1 cargo test`.

use expect_test::expect_file;
use serde_json::Value;

use super::declarations::{ToolDecl, render_server};

/// Every captured payload, with the server name its declarations use.
const CORPUS: &[(&str, &str)] = &[
    (include_str!("testdata/constructs.tools.json"), "constructs"),
    (include_str!("testdata/everything.tools.json"), "everything"),
    (include_str!("testdata/filesystem.tools.json"), "filesystem"),
    (include_str!("testdata/memory.tools.json"), "memory"),
    (include_str!("testdata/playwright.tools.json"), "playwright"),
    (
        include_str!("testdata/sequentialthinking.tools.json"),
        "sequentialthinking",
    ),
];

/// Render a captured `tools/list` payload as one server's declarations.
fn render_corpus(payload: &str, server: &str) -> String {
    let parsed: Value = serde_json::from_str(payload).expect("payload should be valid JSON");
    let tools = parsed["tools"]
        .as_array()
        .expect("payload should carry a `tools` array");

    let decls: Vec<ToolDecl> = tools
        .iter()
        .map(|tool| ToolDecl {
            name: tool["name"].as_str().unwrap_or_default(),
            description: tool["description"].as_str(),
            input_schema: tool.get("inputSchema"),
            output_schema: tool.get("outputSchema"),
        })
        .collect();

    render_server(server, &decls)
}

/// Every corpus entry renders something for every tool, and nothing panics.
///
/// The per-server snapshots below cover the content; this covers the
/// invariant that generation never fails, whatever a server sends.
#[test]
fn every_tool_in_the_corpus_produces_a_declaration() {
    for (payload, server) in CORPUS {
        let parsed: Value = serde_json::from_str(payload).unwrap();
        let expected = parsed["tools"].as_array().unwrap().len();
        let rendered = render_corpus(payload, server);
        // Return-type agnostic: a tool that declares an output schema renders
        // `Promise<{ ... }>` rather than `Promise<unknown>`.
        let found = rendered.matches("): Promise<").count();
        assert!(
            found >= expected,
            "{server}: expected at least {expected} declarations, found {found}"
        );
    }
}

/// The protocol's own test server. Names 12 of its 13 tools with hyphens, so
/// this is also the identifier-handling snapshot.
#[test]
fn everything_server() {
    expect_file!["testdata/everything.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/everything.tools.json"),
        "everything",
    ));
}

/// Carries the corpus's only union-with-literal and declares output schemas
/// on every tool.
#[test]
fn filesystem_server() {
    expect_file!["testdata/filesystem.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/filesystem.tools.json"),
        "filesystem",
    ));
}

/// Deeply nested schemas: the most schema nodes per byte in the corpus.
#[test]
fn memory_server() {
    expect_file!["testdata/memory.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/memory.tools.json"),
        "memory",
    ));
}

/// The 2020-12 dialect, and the corpus's only typed `additionalProperties`
/// and `propertyNames`.
#[test]
fn playwright_server() {
    expect_file!["testdata/playwright.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/playwright.tools.json"),
        "playwright",
    ));
}

/// One tool with a very large description — the size-control case, where the
/// payload is dominated by prose rather than by schema.
#[test]
fn sequentialthinking_server() {
    expect_file!["testdata/sequentialthinking.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/sequentialthinking.tools.json"),
        "sequentialthinking",
    ));
}

/// `$ref`/`$defs`, the composition keywords, tuples and type-less schemas —
/// what pydantic, zod and schemars emit for an ordinary model, and the one
/// payload here that is hand-built rather than captured.
#[test]
fn construct_coverage() {
    expect_file!["testdata/constructs.d.ts"].assert_eq(&render_corpus(
        include_str!("testdata/constructs.tools.json"),
        "constructs",
    ));
}

/// The generated declarations have to parse: `expect_file!` locks the bytes,
/// not whether a compiler accepts them.
///
/// `tsc` stops at syntax errors and never reaches type checking, so a
/// declaration that parses with the wrong shape passes here — the snapshots
/// above are what cover that.
///
/// Skips when no TypeScript is reachable; CI installs Node so it does not
/// skip there.
#[test]
fn declarations_type_check() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut files = Vec::new();
    for (payload, server) in CORPUS {
        // One file per server: two servers may legitimately name a type alike.
        let name = format!("{server}.d.ts");
        std::fs::write(dir.path().join(&name), render_corpus(payload, server))
            .expect("write declarations");
        files.push(name);
    }

    let run = std::process::Command::new("npx")
        .args([
            "-y",
            "-p",
            "typescript@5",
            "tsc",
            "--noEmit",
            "--strict",
            "--skipLibCheck",
        ])
        .args(&files)
        .current_dir(dir.path())
        .output();

    let Ok(output) = run else {
        eprintln!("skipping declarations_type_check: no npx on PATH");
        return;
    };
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // A failure carrying no `error TS<code>` is the toolchain not running
    // (no network for the download, say), not a type error.
    if !output.status.success() && !report.contains("error TS") {
        eprintln!("skipping declarations_type_check: tsc did not run:\n{report}");
        return;
    }
    assert!(
        output.status.success(),
        "generated declarations do not type-check:\n{report}"
    );
}
