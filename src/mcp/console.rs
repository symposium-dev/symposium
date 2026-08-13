//! Capturing what a script logs.
//!
//! Scripts are written blind — the model cannot step through one — so
//! `console.log` is the only way it can see what happened on the way to a
//! result. The output is captured rather than printed: the process speaks
//! JSON-RPC on stdout, and anything written there would corrupt the stream.
//!
//! The buffer is bounded. A loop that logs on every iteration would otherwise
//! fill the agent's context with noise, which is the cost this whole design
//! exists to avoid.

use std::cell::RefCell;
use std::rc::Rc;

use rquickjs::function::Rest;
use rquickjs::{CatchResultExt, Ctx, Function, Object};

/// Console output from one script.
#[derive(Debug, Default)]
pub struct Log {
    entries: Vec<String>,
    bytes: usize,
    limit: usize,
    dropped: bool,
}

/// Shared with the host functions installed in the context.
///
/// `Rc` over `Arc` since the engine and its closures live on one thread.
pub type Shared = Rc<RefCell<Log>>;

impl Log {
    pub fn shared(limit: usize) -> Shared {
        Rc::new(RefCell::new(Self {
            limit,
            ..Self::default()
        }))
    }

    fn push(&mut self, line: String) {
        if self.bytes + line.len() > self.limit {
            self.dropped = true;
            return;
        }
        self.bytes += line.len();
        self.entries.push(line);
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Whether anything was discarded for want of budget.
    pub fn dropped(&self) -> bool {
        self.dropped
    }
}

/// Install a `console` object backed by `log`.
pub fn install<'js>(ctx: &Ctx<'js>, log: &Shared) -> Result<(), String> {
    let console = Object::new(ctx.clone())
        .catch(ctx)
        .map_err(|e| e.to_string())?;

    // `debug` and `trace` are deliberately included: a model reaching for
    // them should not hit an undefined method mid-script.
    for (method, prefix) in [
        ("log", ""),
        ("info", ""),
        ("debug", ""),
        ("trace", ""),
        ("warn", "[warn] "),
        ("error", "[error] "),
    ] {
        let log = Rc::clone(log);
        let prefix = prefix.to_string();
        let function = Function::new(
            ctx.clone(),
            move |ctx: Ctx<'js>, args: Rest<rquickjs::Value<'js>>| {
                let line = format!("{prefix}{}", format_args_for_log(&ctx, &args.0));
                log.borrow_mut().push(line);
            },
        )
        .catch(ctx)
        .map_err(|e| e.to_string())?;

        console
            .set(method, function)
            .catch(ctx)
            .map_err(|e| e.to_string())?;
    }

    ctx.globals()
        .set("console", console)
        .catch(ctx)
        .map_err(|e| e.to_string())
}

/// Render call arguments the way a developer console would: strings bare,
/// everything else as JSON.
fn format_args_for_log<'js>(ctx: &Ctx<'js>, args: &[rquickjs::Value<'js>]) -> String {
    args.iter()
        .map(|value| {
            if let Some(text) = value.as_string() {
                return text.to_string().unwrap_or_default();
            }
            if value.is_undefined() {
                return "undefined".to_string();
            }
            match ctx.json_stringify(value.clone()) {
                Ok(Some(encoded)) => encoded.to_string().unwrap_or_default(),
                // A value with no JSON form, such as a function.
                _ => "undefined".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use crate::mcp::sandbox::{Limits, Sandbox};
    use std::time::Duration;

    async fn logs_of(source: &str, limit: usize) -> (Vec<String>, bool) {
        let sandbox = Sandbox::new(Limits {
            timeout: Duration::from_secs(5),
            max_console_bytes: limit,
            ..Limits::default()
        });
        let outcome = sandbox.run_script(source).await.unwrap();
        (outcome.logs, outcome.logs_dropped)
    }

    #[tokio::test]
    async fn captures_log_output_in_order() {
        let (logs, dropped) =
            logs_of(r#"console.log("a"); console.log("b"); return 1;"#, 1024).await;
        assert_eq!(logs, vec!["a".to_string(), "b".to_string()]);
        assert!(!dropped);
    }

    #[tokio::test]
    async fn renders_values_as_json_and_strings_bare() {
        let (logs, _) = logs_of(
            r#"console.log("n =", 1, { a: [true, null] }); return 0;"#,
            1024,
        )
        .await;
        assert_eq!(logs, vec![r#"n = 1 {"a":[true,null]}"#.to_string()]);
    }

    #[tokio::test]
    async fn marks_severity_for_warn_and_error() {
        let (logs, _) = logs_of(r#"console.warn("w"); console.error("e"); return 0;"#, 1024).await;
        assert_eq!(logs, vec!["[warn] w".to_string(), "[error] e".to_string()]);
    }

    /// A logging loop must not be able to fill the agent's context.
    #[tokio::test]
    async fn output_is_bounded() {
        let (logs, dropped) = logs_of(
            r#"for (let i = 0; i < 10000; i++) console.log("noise"); return "done";"#,
            64,
        )
        .await;
        assert!(dropped, "the budget should have been exhausted");
        assert!(
            logs.join("").len() <= 64,
            "kept {} bytes, over budget",
            logs.join("").len()
        );
    }

    /// Exhausting the budget bounds the output, not the script.
    #[tokio::test]
    async fn exhausted_budget_does_not_fail_the_script() {
        let sandbox = Sandbox::new(Limits {
            timeout: Duration::from_secs(5),
            max_console_bytes: 8,
            ..Limits::default()
        });
        let outcome = sandbox
            .run_script(r#"console.log("a much longer line than the budget"); return 42;"#)
            .await
            .unwrap();
        assert_eq!(outcome.json, "42");
        assert!(outcome.logs_dropped);
    }

    #[tokio::test]
    async fn console_methods_a_model_may_reach_for_all_exist() {
        let (logs, _) = logs_of(
            r#"["log","info","debug","trace","warn","error"].forEach(m => console[m]("x")); return 0;"#,
            1024,
        )
        .await;
        assert_eq!(logs.len(), 6, "got: {logs:?}");
    }
}
