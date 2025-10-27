//! Symposium Component Protocol (SCP)
//!
//! SCP extends ACP to enable composable agent architectures through proxy chains.
//! Each proxy in the chain can intercept and transform messages, adding capabilities
//! like walkthroughs, collaboration patterns, and IDE integrations.

mod acp;
mod capabilities;
mod jsonrpc;
mod mcp_over_acp;
mod proxy;
mod typed;
pub mod util;

pub use acp::*;
pub use capabilities::*;
pub use jsonrpc::*;
pub use mcp_over_acp::*;
pub use proxy::*;
pub use typed::*;
