//! Turning model-written source into something the engine can evaluate.
//!
//! A model handed TypeScript declarations tends to reply with TypeScript, or
//! with a fenced code block, or with a bare statement body that has no value.
//! QuickJS accepts none of those. Without this step the feature fails on the
//! first script anyone writes.
//!
//! Two forms have to work and cannot be told apart without parsing:
//!
//! ```text
//! await sqlx.query({ sql: "..." })      // an expression, whose value is the result
//! const r = await sqlx.query(...);      // a statement body, whose value is its `return`
//! return r.rows;
//! ```
//!
//! Rather than parse JavaScript in Rust, the generated program tries the
//! expression form and falls back to the statement form, using `new Function`
//! to compile each without running it. The engine's own parser decides.

/// Global the source is handed over on.
///
/// The program reads it once and deletes it, so a script never sees it.
pub const SOURCE_GLOBAL: &str = "__symposiumSource";

/// The program that runs a model-written script.
///
/// Deliberately a constant with no interpolation. The source arrives as a
/// JavaScript value set by the host, not as text formatted into this string,
/// so there is no position in which a caller-supplied value could become
/// code. Keeping it `&'static str` means adding one later cannot be done
/// quietly — the type has to change first.
pub const PROGRAM: &str = r#"(async () => {
  const __src = globalThis.__symposiumSource;
  // Out of reach before any model-written code runs.
  delete globalThis.__symposiumSource;

  let __run;
  try {
    // Concise body: the value is the expression itself, and `await` is in
    // scope because the wrapper is async.
    __run = new Function("return (async () => (" + __src + "\n))();");
  } catch (e1) {
    try {
      // Block body: the value is whatever the source returns.
      __run = new Function("return (async () => {" + __src + "\n})();");
    } catch (e2) {
      throw new SyntaxError(
        "script did not parse as JavaScript (" + e2.message + "). " +
        "Write plain JavaScript: no type annotations, interfaces, or generics."
      );
    }
  }

  // Settle first: the expression form may evaluate to a function the model
  // meant to be called, as in `async () => { ... }`, and that is only
  // visible once the wrapper's own promise resolves.
  const __value = await __run();
  return typeof __value === "function" ? await __value() : __value;
})()"#;

/// Remove the packaging a model puts around code: markdown fences and a
/// module export.
pub fn prepare(source: &str) -> String {
    let mut text = strip_fence(source.trim());
    for prefix in ["export default ", "export default\n"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim().to_string();
            break;
        }
    }
    // A module export is often written as a statement, leaving a stray
    // terminator once the keyword is gone.
    text.trim().trim_end_matches(';').trim().to_string()
}

fn strip_fence(text: &str) -> String {
    let Some(rest) = text.strip_prefix("```") else {
        return text.to_string();
    };
    // The opening fence may carry a language tag, which runs to end of line.
    let body = match rest.split_once('\n') {
        Some((_lang, body)) => body,
        // A fence with no newline has no code in it.
        None => return String::new(),
    };
    body.trim_end()
        .strip_suffix("```")
        .unwrap_or(body)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use crate::mcp::sandbox::{Limits, Sandbox};
    use serde_json::{Value, json};
    use std::time::Duration;

    /// Normalization is only meaningful through the engine, so the tests run
    /// what they produce.
    async fn run(source: &str) -> Result<Value, String> {
        let sandbox = Sandbox::new(Limits {
            timeout: Duration::from_secs(2),
            ..Limits::default()
        });
        sandbox
            .run_script(source)
            .await
            .map_err(|e| e.to_string())?
            .value()
            .map_err(|e| e.to_string())
    }

    // -- the two forms --

    #[tokio::test]
    async fn evaluates_a_bare_expression() {
        assert_eq!(run("1 + 1").await.unwrap(), json!(2));
    }

    #[tokio::test]
    async fn evaluates_a_statement_body() {
        let out = run("const a = 20; const b = 22; return a + b;")
            .await
            .unwrap();
        assert_eq!(out, json!(42));
    }

    /// A model told to write an async arrow writes one; it has to be called
    /// rather than returned as a function.
    #[tokio::test]
    async fn calls_an_async_arrow() {
        assert_eq!(run("async () => 42").await.unwrap(), json!(42));
        assert_eq!(
            run("async () => { return { ok: true }; }").await.unwrap(),
            json!({"ok": true})
        );
    }

    #[tokio::test]
    async fn awaits_inside_either_form() {
        assert_eq!(
            run("await Promise.resolve(7)").await.unwrap(),
            json!(7),
            "expression form"
        );
        assert_eq!(
            run("const v = await Promise.resolve(7); return v * 2;")
                .await
                .unwrap(),
            json!(14),
            "statement form"
        );
    }

    /// An object literal at the start of a statement body is a block, not a
    /// value, so the expression form must be tried first.
    #[tokio::test]
    async fn object_literal_is_not_read_as_a_block() {
        assert_eq!(run("({ a: 1 })").await.unwrap(), json!({"a": 1}));
    }

    // -- packaging --

    #[tokio::test]
    async fn strips_fenced_code_blocks() {
        for fenced in [
            "```\n1 + 1\n```",
            "```js\n1 + 1\n```",
            "```javascript\n1 + 1\n```",
            "```typescript\n1 + 1\n```",
        ] {
            assert_eq!(run(fenced).await.unwrap(), json!(2), "failed on: {fenced}");
        }
    }

    #[tokio::test]
    async fn strips_export_default() {
        assert_eq!(run("export default async () => 5").await.unwrap(), json!(5));
    }

    #[tokio::test]
    async fn strips_a_fence_around_an_export() {
        let out = run("```js\nexport default async () => {\n  return 9;\n}\n```")
            .await
            .unwrap();
        assert_eq!(out, json!(9));
    }

    // -- failure --

    /// The predicted failure: the model is shown TypeScript declarations and
    /// replies in TypeScript, which QuickJS cannot parse. The message has to
    /// say so, or the model has nothing to correct.
    #[tokio::test]
    async fn typescript_syntax_is_reported_clearly() {
        let err = run("async (name: string) => name.length")
            .await
            .unwrap_err();
        assert!(
            err.contains("no type annotations"),
            "error should name the likely cause, got: {err}"
        );
    }

    #[tokio::test]
    async fn runtime_errors_propagate() {
        let err = run(r#"throw new Error("boom")"#).await.unwrap_err();
        assert!(err.contains("boom"), "got: {err}");
    }

    /// An empty script is not an error; it simply has no value.
    #[tokio::test]
    async fn empty_source_yields_null() {
        assert_eq!(run("").await.unwrap(), Value::Null);
        assert_eq!(run("```js\n```").await.unwrap(), Value::Null);
    }

    // -- embedding safety --

    /// The wrapper's own bindings live in a closure. A `new Function` body
    /// sees globals only, never the enclosing lexical scope, so a script
    /// cannot read or overwrite them.
    #[tokio::test]
    async fn wrapper_locals_are_not_visible_to_the_script() {
        for probe in ["typeof __src", "typeof __run", "typeof __value"] {
            assert_eq!(run(probe).await.unwrap(), json!("undefined"), "{probe}");
        }
    }

    /// These are legal raw in a JSON string but were illegal raw in a
    /// JavaScript string literal until ES2019, so the embedding depends on
    /// the engine accepting them.
    #[tokio::test]
    async fn line_separators_survive_embedding() {
        let out = run("return \"a\u{2028}b\u{2029}c\";").await.unwrap();
        assert_eq!(out, json!("a\u{2028}b\u{2029}c"));
    }

    /// A script can garble the wrapper's structure — it is concatenated into
    /// a source string and compiled, which is the whole point. What matters
    /// is that doing so crosses no boundary: the escaped code runs in the
    /// same sandbox, with the same absence of host capabilities.
    #[tokio::test]
    async fn escaping_the_wrapper_reaches_no_capabilities() {
        let out = run(r#"1)); globalThis.reached = typeof fetch; ((async () => (2"#).await;
        match out {
            Err(e) => assert!(e.contains("did not parse"), "got: {e}"),
            Ok(_) => {
                let probe = run("typeof fetch").await.unwrap();
                assert_eq!(probe, json!("undefined"));
            }
        }
    }

    // -- escaping --

    /// The source is embedded in the generated program as a string literal,
    /// so quotes, backslashes and newlines in the model's code must survive.
    #[tokio::test]
    async fn source_containing_quotes_survives_embedding() {
        let out = run(r#"return "he said \"hi\"\n";"#).await.unwrap();
        assert_eq!(out, json!("he said \"hi\"\n"));
    }

    #[tokio::test]
    async fn source_containing_a_template_literal_survives() {
        let out = run("const n = 2; return `n is ${n}`;").await.unwrap();
        assert_eq!(out, json!("n is 2"));
    }
}
