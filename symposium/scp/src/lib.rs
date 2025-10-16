//! Symposium Component Protocol (SCP)
//!
//! SCP extends ACP to enable composable agent architectures through proxy chains.
//! Each proxy in the chain can intercept and transform messages, adding capabilities
//! like walkthroughs, collaboration patterns, and IDE integrations.

pub mod jsonrpc;

pub mod acp;
mod util;
