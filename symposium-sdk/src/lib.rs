//! SDK for writing symposium plugins in Rust.
//!
//! This crate is the single dependency for plugin binaries — hooks, custom
//! predicates, and subcommands. It re-exports the hook handler SDK and adds
//! types for other plugin interfaces.
//!
//! # Hooks
//!
//! ```no_run
//! use std::process::ExitCode;
//! use symposium_sdk::hook::{HookHandler, PreToolUseInput, PreToolUseOutput, run};
//!
//! struct MyHook;
//!
//! impl HookHandler for MyHook {
//!     async fn pre_tool_use(
//!         &self,
//!         _event: &PreToolUseInput,
//!     ) -> symposium_sdk::hook::anyhow::Result<PreToolUseOutput> {
//!         Ok(PreToolUseOutput::default())
//!     }
//! }
//!
//! fn main() -> ExitCode {
//!     run(MyHook)
//! }
//! ```
//!
//! # Custom predicates
//!
//! A custom predicate binary receives its argument via CLI args, and signals
//! pass/fail via exit code. To participate in crate-sourced skill resolution,
//! it prints witness JSON to stdout:
//!
//! ```no_run
//! use symposium_sdk::predicate::{PredicateOutput, SelectedCrate};
//!
//! let output = PredicateOutput {
//!     selected_crates: vec![
//!         SelectedCrate {
//!             crate_name: "my-crate".into(),
//!             version: semver::Version::new(1, 0, 0),
//!         },
//!     ],
//! };
//! println!("{}", serde_json::to_string(&output).unwrap());
//! ```

pub use symposium_hook as hook;

pub mod dirs;
pub mod predicate;
pub mod workspace;
