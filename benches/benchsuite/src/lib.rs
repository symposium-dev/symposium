//! Shared fixture and sandbox support for Symposium benchmarks.

mod cargo;
mod fixture;
mod sandbox;

pub use cargo::MetadataRejectingCargo;
pub use fixture::{Fixture, StagedFixture};
pub use sandbox::Sandbox;
