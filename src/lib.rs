pub mod agents;
pub mod cli;
pub mod config;
pub mod crate_command;
pub mod help_render;
pub mod hook;
pub mod hook_schema;
pub(crate) mod installation;
pub mod output;
pub mod plugins;
pub mod report;
pub mod self_update;
pub mod state;
pub mod subcommand_dispatch;
pub mod workspace_state;

pub(crate) mod crate_metadata;
pub(crate) mod init;
pub mod sync;

pub(crate) mod crate_sources;
pub(crate) mod predicate;
pub(crate) mod shell_predicate;
pub(crate) mod skills;

#[cfg(test)]
pub(crate) mod test_utils;
