//! Proxy support for the Symposium Component Protocol (S/ACP).
//!
//! This module provides utilities for building proxy components that sit between
//! editors and agents in an S/ACP chain. Proxies can intercept, transform, and
//! forward messages in both directions.
//!
//! # Core Concepts
//!
//! ## Message Flow
//!
//! In an S/ACP chain, messages flow through proxies:
//!
//! ```text
//! Editor → Proxy → Agent
//!        ↓      ↓
//!    (upstream) (downstream/successor)
//! ```
//!
//! - **Upstream**: Messages from/to the editor
//! - **Downstream/Successor**: Messages from/to the next component (another proxy or agent)
//!
//! ## Handler Abstraction
//!
//! The [`FromProxyHandler`] wrapper allows proxy authors to write handlers that
//! process normal ACP messages without dealing with the `_proxy/successor/receive/*`
//! protocol wrappers. The handler automatically unwraps incoming messages from
//! successors and rewraps responses.
//!
//! # Example
//!
//! ```rust,ignore
//! use scp::proxy::JsonRpcConnectionExt;
//! use scp::{JsonRpcConnection, JsonRpcHandler};
//!
//! // Your handler processes normal ACP messages
//! struct MyProxyHandler;
//! impl JsonRpcHandler for MyProxyHandler {
//!     // Handle requests and notifications like any ACP component
//! }
//!
//! # async fn example() -> Result<(), jsonrpcmsg::Error> {
//! JsonRpcConnection::new(tokio::io::stdin(), tokio::io::stdout())
//!     .on_receive_from_successor(MyProxyHandler)
//!     .serve()
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod conductor;
mod messages;
mod on_receive_from_successor;
mod send_request_to_successor;

pub use conductor::*;
pub use messages::*;
pub use on_receive_from_successor::*;
pub use send_request_to_successor::*;
