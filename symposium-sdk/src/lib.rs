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
//! pass/fail via exit code. It may also stream [`CustomPredicateEvent`]s on
//! stdout to describe the inputs whose changes should invalidate its cached
//! result. Prefer the [`env::var`] and [`fs::read_to_string`] helpers, which
//! report their inputs automatically.
//!
//! [`CustomPredicateEvent`]: predicate::CustomPredicateEvent
//!
//! # Package managers
//!
//! [`manifest`] is the `SYMPOSIUM.toml` schema and [`pm`] the package
//! identity. Together they are what crosses the package-manager boundary: a PM
//! answers with a [`pm::PackageId`], a content directory, and a
//! [`manifest::RawPluginManifest`] it parsed, translated, or synthesized.
//! Validating that manifest (defaults, dormancy, trust) is Symposium's job,
//! not the PM's.

pub mod env;
pub mod fs;
pub mod hook;
pub mod manifest;
pub mod pm;
pub mod predicate;
