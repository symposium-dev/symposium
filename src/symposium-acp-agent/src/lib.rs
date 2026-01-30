//! Symposium ACP Agent library
//!
//! This crate provides the Symposium proxy chain orchestration and the VS Code
//! Language Model Provider backend.

pub mod config_agent;
pub mod recommendations;
pub mod registry;
pub mod remote_recommendations;
pub mod symposium;
pub mod user_config;
pub mod vscodelm;

pub use config_agent::ConfigAgent;
