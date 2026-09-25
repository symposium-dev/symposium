//! Attribution supplied to extension-invocation admission.

use super::super::resolution::extension::SafeSkillAttribution;
use super::vocabulary::UnnamedExtensionReason;

/// Attribution result supplied for one normalized skill invocation.
///
/// Overflow is deliberately absent. Private state assigns overflow only after
/// the daily public-row allowance is exhausted, so producers cannot bypass
/// admission or discard otherwise nameable public attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionInvocationAttribution {
    Public(SafeSkillAttribution),
    Unnamed(UnnamedExtensionReason),
}
