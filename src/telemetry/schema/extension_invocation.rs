//! Extension-invocation aggregate schema and supporting types.

mod bucket;
mod row;
mod vocabulary;

pub(in crate::telemetry) use bucket::ExtensionInvocationAttribution;
pub(in crate::telemetry) use row::{
    ExtensionInvocationMetricObservation, ExtensionInvocationMetricsUpdateError,
    ExtensionInvocationMetricsV1,
};
pub(in crate::telemetry) use vocabulary::{
    ExtensionInvocationAgent, ExtensionInvocationPhase, ExtensionTargetScope,
    UnnamedExtensionReason,
};
