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
//! it emits JSON Lines records to stdout using [`PredicateEmitter`]:
//!
//! ```no_run
//! use symposium_sdk::predicate::PredicateEmitter;
//!
//! PredicateEmitter::stdout()
//!     .selected_crate("my-crate", &semver::Version::new(1, 0, 0))
//!     .unwrap();
//! ```

pub mod dirs;
pub mod hook;
pub mod predicate;
pub mod workspace;
