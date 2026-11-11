//! Audits: statements about the plugin payload a package ships.
//!
//! The audits file format lives in [`symposium_sdk::audits`]. This module
//! computes the payload digest an audits file matches a package on
//! ([`digest`]) and backs `cargo agents plugin digest` ([`digest_command`]).

pub mod digest;
pub mod digest_command;
