//! Shared private-state outcomes for cumulative telemetry rows.

/// Result of consuming one aggregate private-state stage.
///
/// Committing remains infallible because one recording operation applies
/// several stores sequentially. A stage poisoned by failed selection instead
/// reports that it discarded every staged edit.
#[must_use = "aggregate stage commit outcomes must be observed"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum StageCommit {
    /// Every staged edit was applied to its destination store.
    Applied,
    /// A selection failed, so every staged edit was discarded.
    DiscardedPoisoned,
}
