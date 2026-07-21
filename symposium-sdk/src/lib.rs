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
//! A custom predicate binary receives its argument via CLI args and signals
//! pass/fail via exit code — that is the whole contract today.
//!
//! FIXME: [`predicate::PredicateEmitter`] is a reserved stdout channel for a
//! custom predicate to set fields on the plugin (or component) it gates. It was
//! originally built to name crates for the retired `source = "crate"`
//! resolution and is currently ignored; see the [`predicate`] module docs.

pub mod dirs;
pub mod hook;
pub mod predicate;
pub mod workspace;
