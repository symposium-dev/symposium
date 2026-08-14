//! The MCP meta-server.
//!
//! Symposium registers a single MCP server with each configured agent. That
//! server exposes the tools of every applicable plugin MCP server, instead of
//! each plugin being registered separately.
//!
//! See the [MCP meta-server RFD](../../md/rfds/mcp-meta-server/README.md).

pub mod catalog;
pub mod client;
pub mod console;
pub mod credentials;
pub mod declarations;
pub mod dispatch;
pub mod endpoint;
pub mod login;
pub mod normalize;
pub mod resolve;
pub mod sandbox;
pub mod schema_to_ts;
pub mod server;
pub mod supervisor;
pub mod validate;

#[cfg(test)]
mod corpus_tests;
