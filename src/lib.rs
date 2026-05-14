use std::process::Command;

pub mod agents;
pub mod cli;
pub mod config;
pub mod crate_command;
pub mod hook;
pub mod hook_schema;
pub mod init;
pub mod output;
pub mod plugins;
pub mod self_update;
pub mod state;
pub mod sync;

pub(crate) mod crate_sources;
pub(crate) mod installation;
pub(crate) mod predicate;
pub(crate) mod skills;

#[cfg(test)]
pub(crate) mod test_utils;

/// Build a `Command` for the cargo binary, falling back to `"cargo"`.
pub fn cargo_command() -> Command {
    Command::new("cargo")
}
