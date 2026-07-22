pub mod agents;
pub mod cli;
pub mod config;
pub mod crate_command;
pub mod discovery;
pub mod help_render;
pub mod hook;
pub mod hook_schema;
pub(crate) mod installation;
pub mod output;
pub mod plugins;
pub mod pm;
pub mod report;
pub mod search_command;
pub mod self_update;
pub mod state;
pub mod status_command;
pub mod subcommand_dispatch;
pub mod telemetry;
pub mod use_command;
pub mod workspace_state;

pub(crate) mod crate_metadata;
pub(crate) mod init;
pub mod sync;

pub(crate) mod crate_sources;
pub(crate) mod predicate;
pub(crate) mod skills;

pub use symposium_install::UpdateLevel;

#[cfg(test)]
pub(crate) mod test_utils;
