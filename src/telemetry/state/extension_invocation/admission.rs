//! Bounded admission and private selection for invocation aggregates.

use std::{collections::BTreeMap, fmt};

use super::ExtensionSessionCountTracker;
use crate::telemetry::{
    identity::{AgentSubject, ExtensionSubject, IdentifierWindowScope},
    schema::{
        EventId, ExtensionInvocationAgent, ExtensionInvocationAttribution, ExtensionTargetScope,
        PublicSkillCoordinate, SafeSkillAttribution, SupportedAgent, UnnamedExtensionReason,
        UtcDay,
    },
    state::{
        BoundRecordingObservation, DayBeforeCurrent, StageCommit,
        open_day::OpenDayUpdate,
        public_row_budget::{DailyPublicRowBudget, PublicRowAdmission},
        session_pair::TrackedSessionPair,
        staged_entries::StagedEntries,
    },
    storage::metrics::AggregateRecoveryIndex,
};

/// Identity fields admitted for one extension-invocation aggregate row.
///
/// The inner enum is private so only state admission can create an overflow
/// bucket or pair a public target with its scoped subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct AdmittedExtensionBucket(AdmittedExtensionBucketKind);

#[derive(Debug, Clone, PartialEq, Eq)]
enum AdmittedExtensionBucketKind {
    Public {
        attribution: SafeSkillAttribution,
        subject: ExtensionSubject,
    },
    Unnamed(UnnamedExtensionReason),
    Overflow,
}

impl AdmittedExtensionBucket {
    fn from_attribution(
        recording: &BoundRecordingObservation<'_>,
        attribution: ExtensionInvocationAttribution,
    ) -> Self {
        match attribution {
            ExtensionInvocationAttribution::Public(attribution) => {
                Self::public(recording.identifier_window_scope(), attribution)
            }
            ExtensionInvocationAttribution::Unnamed(reason) => Self::unnamed(reason),
        }
    }

    pub(in crate::telemetry::state) fn public(
        scope: &IdentifierWindowScope<'_>,
        attribution: SafeSkillAttribution,
    ) -> Self {
        let subject = attribution.derive_subject(scope);
        Self(AdmittedExtensionBucketKind::Public {
            attribution,
            subject,
        })
    }

    pub(in crate::telemetry::state) const fn unnamed(reason: UnnamedExtensionReason) -> Self {
        Self(AdmittedExtensionBucketKind::Unnamed(reason))
    }

    pub(in crate::telemetry::state) const fn overflow() -> Self {
        Self(AdmittedExtensionBucketKind::Overflow)
    }

    #[must_use]
    pub(in crate::telemetry) const fn scope(&self) -> ExtensionTargetScope {
        match self.0 {
            AdmittedExtensionBucketKind::Public { .. } => ExtensionTargetScope::Public,
            AdmittedExtensionBucketKind::Unnamed(_) => ExtensionTargetScope::Unnamed,
            AdmittedExtensionBucketKind::Overflow => ExtensionTargetScope::Overflow,
        }
    }

    #[must_use]
    pub(in crate::telemetry) const fn target(&self) -> Option<&PublicSkillCoordinate> {
        match &self.0 {
            AdmittedExtensionBucketKind::Public { attribution, .. } => Some(attribution.target()),
            AdmittedExtensionBucketKind::Unnamed(_) | AdmittedExtensionBucketKind::Overflow => None,
        }
    }

    /// Return the validated target and path retained by a public bucket.
    #[must_use]
    pub(in crate::telemetry) const fn safe_attribution(&self) -> Option<&SafeSkillAttribution> {
        match &self.0 {
            AdmittedExtensionBucketKind::Public { attribution, .. } => Some(attribution),
            AdmittedExtensionBucketKind::Unnamed(_) | AdmittedExtensionBucketKind::Overflow => None,
        }
    }

    #[must_use]
    pub(in crate::telemetry) const fn unnamed_reason(&self) -> Option<UnnamedExtensionReason> {
        match self.0 {
            AdmittedExtensionBucketKind::Unnamed(reason) => Some(reason),
            AdmittedExtensionBucketKind::Public { .. } | AdmittedExtensionBucketKind::Overflow => {
                None
            }
        }
    }

    #[must_use]
    pub(in crate::telemetry) const fn extension_subject(&self) -> Option<ExtensionSubject> {
        match self.0 {
            AdmittedExtensionBucketKind::Public { subject, .. } => Some(subject),
            AdmittedExtensionBucketKind::Unnamed(_) | AdmittedExtensionBucketKind::Overflow => None,
        }
    }

    /// Return the target and derived subject of a public bucket together.
    #[must_use]
    const fn public_identity(&self) -> Option<(&PublicSkillCoordinate, ExtensionSubject)> {
        match &self.0 {
            AdmittedExtensionBucketKind::Public {
                attribution,
                subject,
            } => Some((attribution.target(), *subject)),
            AdmittedExtensionBucketKind::Unnamed(_) | AdmittedExtensionBucketKind::Overflow => None,
        }
    }

    pub(in crate::telemetry::state) const fn key(&self) -> ExtensionInvocationBucketKey {
        match self.0 {
            AdmittedExtensionBucketKind::Public { subject, .. } => {
                ExtensionInvocationBucketKey::Public(subject)
            }
            AdmittedExtensionBucketKind::Unnamed(reason) => {
                ExtensionInvocationBucketKey::Unnamed(reason)
            }
            AdmittedExtensionBucketKind::Overflow => ExtensionInvocationBucketKey::Overflow,
        }
    }
}

/// Private lookup key for one daily invocation aggregate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::telemetry) struct ExtensionInvocationAggregateKey {
    pub(in crate::telemetry::state) day: UtcDay,
    // The row needs the plaintext agent; its subject separates private epochs.
    pub(in crate::telemetry::state) agent: ExtensionInvocationAgent,
    pub(in crate::telemetry::state) agent_subject: AgentSubject,
    pub(in crate::telemetry::state) bucket: ExtensionInvocationBucketKey,
}

impl ExtensionInvocationAggregateKey {
    fn new(
        recording: &BoundRecordingObservation<'_>,
        agent: ExtensionInvocationAgent,
        bucket: &AdmittedExtensionBucket,
    ) -> Self {
        Self::from_scope(
            recording.day(),
            recording.identifier_window_scope(),
            agent,
            bucket,
        )
    }

    pub(in crate::telemetry::state) fn from_scope(
        day: UtcDay,
        scope: &IdentifierWindowScope<'_>,
        agent: ExtensionInvocationAgent,
        bucket: &AdmittedExtensionBucket,
    ) -> Self {
        let agent_subject = scope.derive(&SupportedAgent::from(agent));

        Self {
            day,
            agent,
            agent_subject,
            bucket: bucket.key(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::telemetry::state) enum ExtensionInvocationBucketKey {
    Public(ExtensionSubject),
    Unnamed(UnnamedExtensionReason),
    Overflow,
}

/// Private state paired with one extension-invocation aggregate row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry::state) struct ExtensionInvocationAggregateState {
    event_id: EventId,
    bucket: AdmittedExtensionBucket,
    session_counts: ExtensionSessionCountTracker<ExtensionInvocationAggregateKey>,
}

impl ExtensionInvocationAggregateState {
    fn new(key: &ExtensionInvocationAggregateKey, bucket: AdmittedExtensionBucket) -> Self {
        Self::with_event_id(key, bucket, EventId::new())
    }

    /// Recover private state for a surviving public snapshot row.
    ///
    /// The fresh tracker deliberately disagrees with a non-empty row's
    /// contribution baseline. Its first later update therefore marks both
    /// session sets incomplete while aggregate totals continue from the row.
    fn recovered(
        key: &ExtensionInvocationAggregateKey,
        bucket: AdmittedExtensionBucket,
        event_id: EventId,
    ) -> Self {
        Self::with_event_id(key, bucket, event_id)
    }

    fn with_event_id(
        key: &ExtensionInvocationAggregateKey,
        bucket: AdmittedExtensionBucket,
        event_id: EventId,
    ) -> Self {
        Self {
            event_id,
            bucket,
            session_counts: ExtensionSessionCountTracker::new(key.clone()),
        }
    }

    /// Borrow the private row identity and session tracker for persistence.
    #[must_use]
    pub(in crate::telemetry::state) const fn persistence_parts(
        &self,
    ) -> (
        EventId,
        &AdmittedExtensionBucket,
        &ExtensionSessionCountTracker<ExtensionInvocationAggregateKey>,
    ) {
        (self.event_id, &self.bucket, &self.session_counts)
    }

    /// Rebuild an entry from decoded private-state fields.
    ///
    /// The persistence caller must validate the containing store before
    /// returning it; construction alone does not prove identity consistency.
    #[must_use]
    pub(in crate::telemetry::state) fn from_persisted(
        key: &ExtensionInvocationAggregateKey,
        event_id: EventId,
        bucket: AdmittedExtensionBucket,
        attempted_contributions: u64,
        completed_contributions: u64,
        sessions: TrackedSessionPair,
    ) -> Self {
        Self {
            event_id,
            bucket,
            session_counts: ExtensionSessionCountTracker::from_persisted(
                key.clone(),
                attempted_contributions,
                completed_contributions,
                sessions,
            ),
        }
    }

    fn select(
        &mut self,
        key: &ExtensionInvocationAggregateKey,
    ) -> Result<SelectedExtensionInvocationAggregate<'_>, ExtensionAggregateSelectionError> {
        if self.session_counts.key() != key {
            return Err(ExtensionAggregateSelectionError);
        }

        self.select_admitted()
    }

    fn select_admitted(
        &mut self,
    ) -> Result<SelectedExtensionInvocationAggregate<'_>, ExtensionAggregateSelectionError> {
        if self.bucket.key() != self.session_counts.key().bucket {
            return Err(ExtensionAggregateSelectionError);
        }

        Ok(SelectedExtensionInvocationAggregate {
            event_id: self.event_id,
            bucket: &self.bucket,
            session_counts: &mut self.session_counts,
        })
    }
}

/// Key-checked access to one admitted extension-invocation aggregate.
pub(in crate::telemetry) struct SelectedExtensionInvocationAggregate<'a> {
    event_id: EventId,
    bucket: &'a AdmittedExtensionBucket,
    session_counts: &'a mut ExtensionSessionCountTracker<ExtensionInvocationAggregateKey>,
}

impl SelectedExtensionInvocationAggregate<'_> {
    #[must_use]
    pub(in crate::telemetry) const fn event_id(&self) -> EventId {
        self.event_id
    }

    #[must_use]
    pub(in crate::telemetry) const fn day(&self) -> UtcDay {
        self.session_counts.key().day
    }

    #[must_use]
    pub(in crate::telemetry) const fn agent(&self) -> ExtensionInvocationAgent {
        self.session_counts.key().agent
    }

    #[must_use]
    pub(in crate::telemetry) const fn bucket(&self) -> &AdmittedExtensionBucket {
        self.bucket
    }

    /// Return whether this selection belongs to the supplied recording
    /// context, including its identifier epoch.
    #[must_use]
    pub(in crate::telemetry) fn matches_recording(
        &self,
        recording: &BoundRecordingObservation<'_>,
    ) -> bool {
        let key = self.session_counts.key();
        key.day == recording.day()
            && key.agent_subject
                == recording
                    .identifier_window_scope()
                    .derive(&SupportedAgent::from(key.agent))
    }

    #[must_use]
    pub(in crate::telemetry) const fn session_counts(
        &mut self,
    ) -> &mut ExtensionSessionCountTracker<ExtensionInvocationAggregateKey> {
        self.session_counts
    }
}

/// Daily private state for extension-invocation aggregate admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionInvocationAggregateStore {
    public_rows: DailyPublicRowBudget,
    entries: BTreeMap<ExtensionInvocationAggregateKey, ExtensionInvocationAggregateState>,
}

impl ExtensionInvocationAggregateStore {
    #[must_use]
    pub(in crate::telemetry) const fn new(day: UtcDay) -> Self {
        Self {
            public_rows: DailyPublicRowBudget::new(day),
            entries: BTreeMap::new(),
        }
    }

    /// Return the UTC day owned by this private aggregate store.
    #[must_use]
    pub(in crate::telemetry::state) const fn day(&self) -> UtcDay {
        self.public_rows.persistence_parts().0
    }

    /// Return how many public-row slots this store has spent today.
    #[must_use]
    pub(in crate::telemetry::state) const fn public_rows_spent(&self) -> u64 {
        self.public_rows.persistence_parts().1
    }

    /// Borrow entries in their deterministic persistence order.
    pub(in crate::telemetry::state) fn persistence_entries(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            &ExtensionInvocationAggregateKey,
            &ExtensionInvocationAggregateState,
        ),
    > {
        self.entries.iter()
    }

    /// Rebuild a store from decoded private-state fields.
    ///
    /// The persistence caller must run its store-level validator before
    /// returning this value; construction alone does not establish allowance
    /// or entry-identity consistency.
    #[must_use]
    pub(in crate::telemetry::state) const fn from_persisted(
        day: UtcDay,
        public_rows_spent: u64,
        entries: BTreeMap<ExtensionInvocationAggregateKey, ExtensionInvocationAggregateState>,
    ) -> Self {
        Self {
            public_rows: DailyPublicRowBudget::from_persisted(day, public_rows_spent),
            entries,
        }
    }

    /// Stage private-state edits for one recording operation.
    ///
    /// The required recovery index proves that the day's snapshot loaded
    /// successfully. Every surviving public row contributes to the reconciled
    /// allowance. Day selection and allowance reconciliation happen on a copy,
    /// so dropping the returned stage leaves this store unchanged. Access is
    /// restricted to private state; row-level tests use the narrower test shim.
    ///
    /// # Errors
    ///
    /// Returns an admission error if snapshot and observation days disagree,
    /// or the observation day moves backward.
    pub(in crate::telemetry::state) fn stage<'store, 'context, 'identity>(
        &'store mut self,
        recovery: &'context AggregateRecoveryIndex,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<
        ExtensionInvocationAggregateStage<'store, 'context, 'identity>,
        ExtensionInvocationAdmissionError,
    > {
        if recovery.day() != recording.day() {
            return Err(ExtensionInvocationAdmissionError::SnapshotDayMismatch {
                snapshot_day: recovery.day(),
                observation_day: recording.day(),
            });
        }
        let mut staged_budget = self.public_rows;
        let day_update = staged_budget
            .reconcile(recording.day(), recovery.extension_invocation_public_rows())?;

        Ok(ExtensionInvocationAggregateStage {
            destination_budget: &mut self.public_rows,
            staged_budget,
            entries: StagedEntries::new(&mut self.entries, day_update == OpenDayUpdate::Advanced),
            recovery,
            recording,
            poisoned: false,
        })
    }

    /// Widen staged access only for row-level tests outside private state.
    #[cfg(test)]
    pub(in crate::telemetry) fn stage_for_test<'store, 'context, 'identity>(
        &'store mut self,
        recovery: &'context AggregateRecoveryIndex,
        recording: &'context BoundRecordingObservation<'identity>,
    ) -> Result<
        ExtensionInvocationAggregateStage<'store, 'context, 'identity>,
        ExtensionInvocationAdmissionError,
    > {
        self.stage(recovery, recording)
    }

    /// Remove entries from the previous identifier epoch without restoring
    /// the current day's public-row allowance.
    pub(in crate::telemetry) fn reset_identifier_epoch(&mut self) {
        self.entries.clear();
    }

    /// Remove current rows and restore the current day's public-row allowance.
    pub(in crate::telemetry) fn clear(&mut self) {
        self.entries.clear();
        self.public_rows.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Copy-on-write extension-invocation admission for one recording operation.
///
/// A skill-hook operation selects at most one extension row. Repeated
/// selection remains supported so this stage has the same semantics as the
/// other aggregate families.
#[must_use = "dropping an extension-invocation stage rolls back its private-state edits"]
pub(in crate::telemetry) struct ExtensionInvocationAggregateStage<'store, 'context, 'identity> {
    destination_budget: &'store mut DailyPublicRowBudget,
    staged_budget: DailyPublicRowBudget,
    entries:
        StagedEntries<'store, ExtensionInvocationAggregateKey, ExtensionInvocationAggregateState>,
    recovery: &'context AggregateRecoveryIndex,
    recording: &'context BoundRecordingObservation<'identity>,
    poisoned: bool,
}

impl ExtensionInvocationAggregateStage<'_, '_, '_> {
    /// Return whether a failed selection made this stage unsafe to commit.
    #[must_use]
    pub(in crate::telemetry) const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// Select or admit the aggregate for one attributed observation.
    ///
    /// Adoption happens before overflow and does not spend another public-row
    /// slot. Repeated selections reuse the same staged entry. Any selection
    /// error poisons the stage, so a later commit cannot apply partial
    /// allowance or entry changes even if a caller ignores the error.
    pub(in crate::telemetry) fn select(
        &mut self,
        agent: ExtensionInvocationAgent,
        attribution: ExtensionInvocationAttribution,
    ) -> Result<SelectedExtensionInvocationAggregate<'_>, ExtensionInvocationAdmissionError> {
        let Self {
            staged_budget,
            entries,
            recovery,
            recording,
            poisoned,
            ..
        } = self;
        let result = Self::select_inner(
            staged_budget,
            entries,
            recovery,
            recording,
            agent,
            attribution,
        );
        if result.is_err() {
            *poisoned = true;
        }
        result
    }

    fn select_inner<'a>(
        staged_budget: &mut DailyPublicRowBudget,
        entries: &'a mut StagedEntries<
            '_,
            ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState,
        >,
        recovery: &AggregateRecoveryIndex,
        recording: &BoundRecordingObservation<'_>,
        agent: ExtensionInvocationAgent,
        attribution: ExtensionInvocationAttribution,
    ) -> Result<SelectedExtensionInvocationAggregate<'a>, ExtensionInvocationAdmissionError> {
        let bucket = AdmittedExtensionBucket::from_attribution(recording, attribution);
        let key = ExtensionInvocationAggregateKey::new(recording, agent, &bucket);

        // Returning a selection borrows the overlay. On stable Rust a single
        // mutable lookup would keep that borrow alive across the vacant path,
        // so establish occupancy with a read before borrowing for the return.
        if entries.contains_key(&key) {
            return Self::select_existing(entries, &key);
        }

        if let Some((target, subject)) = bucket.public_identity() {
            if let Some(event_id) = recovery.extension_invocation_event_id(agent, target, subject) {
                return Self::insert_recovered(entries, &key, bucket, event_id);
            }

            if staged_budget.is_exhausted() {
                return Self::select_public_at_capacity(entries, key);
            }

            let PublicRowAdmission::Public = staged_budget.admit_new() else {
                unreachable!("BUG: a non-exhausted public-row budget must admit a row");
            };
        }

        Self::select_or_insert(entries, &key, bucket)
    }

    fn select_public_at_capacity<'a>(
        entries: &'a mut StagedEntries<
            '_,
            ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState,
        >,
        mut key: ExtensionInvocationAggregateKey,
    ) -> Result<SelectedExtensionInvocationAggregate<'a>, ExtensionInvocationAdmissionError> {
        let bucket = AdmittedExtensionBucket::overflow();
        key.bucket = bucket.key();
        Self::select_or_insert(entries, &key, bucket)
    }

    fn select_or_insert<'a>(
        entries: &'a mut StagedEntries<
            '_,
            ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState,
        >,
        key: &ExtensionInvocationAggregateKey,
        bucket: AdmittedExtensionBucket,
    ) -> Result<SelectedExtensionInvocationAggregate<'a>, ExtensionInvocationAdmissionError> {
        let state = entries.get_or_insert_with(key.clone(), |key| {
            ExtensionInvocationAggregateState::new(key, bucket)
        });
        state
            .select(key)
            .map_err(ExtensionInvocationAdmissionError::PrivateState)
    }

    fn select_existing<'a>(
        entries: &'a mut StagedEntries<
            '_,
            ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState,
        >,
        key: &ExtensionInvocationAggregateKey,
    ) -> Result<SelectedExtensionInvocationAggregate<'a>, ExtensionInvocationAdmissionError> {
        let Some(entry) = entries.get_mut(key) else {
            return Err(ExtensionInvocationAdmissionError::PrivateState(
                ExtensionAggregateSelectionError,
            ));
        };
        entry
            .select(key)
            .map_err(ExtensionInvocationAdmissionError::PrivateState)
    }

    fn insert_recovered<'a>(
        entries: &'a mut StagedEntries<
            '_,
            ExtensionInvocationAggregateKey,
            ExtensionInvocationAggregateState,
        >,
        key: &ExtensionInvocationAggregateKey,
        bucket: AdmittedExtensionBucket,
        event_id: EventId,
    ) -> Result<SelectedExtensionInvocationAggregate<'a>, ExtensionInvocationAdmissionError> {
        let state = entries.get_or_insert_with(key.clone(), |key| {
            ExtensionInvocationAggregateState::recovered(key, bucket, event_id)
        });
        state
            .select(key)
            .map_err(ExtensionInvocationAdmissionError::PrivateState)
    }

    /// Apply the staged allowance and entries without a recoverable failure.
    ///
    /// Hook-invocation recording commits this alongside the hook and
    /// plugin-hook stages. A poisoned stage discards all edits and reports
    /// that outcome without making commit fallible.
    pub(in crate::telemetry) fn commit(self) -> StageCommit {
        let Self {
            destination_budget,
            staged_budget,
            entries,
            recovery: _,
            recording: _,
            poisoned,
        } = self;
        if poisoned {
            return StageCommit::DiscardedPoisoned;
        }
        *destination_budget = staged_budget;
        entries.commit();
        StageCommit::Applied
    }
}

/// Failure while selecting private extension-invocation aggregate state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum ExtensionInvocationAdmissionError {
    SnapshotDayMismatch {
        snapshot_day: UtcDay,
        observation_day: UtcDay,
    },
    DayBeforeCurrent(DayBeforeCurrent),
    PrivateState(ExtensionAggregateSelectionError),
}

impl From<DayBeforeCurrent> for ExtensionInvocationAdmissionError {
    fn from(error: DayBeforeCurrent) -> Self {
        Self::DayBeforeCurrent(error)
    }
}

impl fmt::Display for ExtensionInvocationAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotDayMismatch {
                snapshot_day,
                observation_day,
            } => write!(
                formatter,
                "extension-invocation snapshot belongs to {snapshot_day}, not observation day {observation_day}"
            ),
            Self::DayBeforeCurrent(error) => {
                write!(formatter, "extension-invocation budget {error}")
            }
            Self::PrivateState(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ExtensionInvocationAdmissionError {}

/// A private entry selected with another aggregate key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct ExtensionAggregateSelectionError;

impl fmt::Display for ExtensionAggregateSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("extension-invocation private state belongs to another aggregate")
    }
}

impl std::error::Error for ExtensionAggregateSelectionError {}

#[cfg(test)]
mod tests;
