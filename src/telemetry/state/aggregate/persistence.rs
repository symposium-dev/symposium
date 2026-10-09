//! Private wire conversion for aggregate telemetry state.
//!
//! Runtime trackers deliberately do not implement Serde: they contain keyed
//! session identifiers, and a general serializer would make accidental
//! disclosure easy. This module is the only bridge between those runtime
//! values and `telemetry-state.toml`.
//!
//! Borrowed store wire types declare scalar metadata before their entry
//! arrays. TOML renders those arrays as tables and cannot encode a later
//! scalar, reporting `ValueAfterTable` instead.
//!
//! Family validators reject duplicate row identifiers within one store. The
//! root aggregate-state validator owns cross-family identifier uniqueness and
//! agreement between every store day and the identity high-water day.
mod hook;
mod plugin_hook;
mod sessions;

use std::{error::Error, fmt};

use super::AggregateFamily;

/// Content-free reason aggregate private state violates version 1 invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum AggregateStateInvariantError {
    EntryIdentityMismatch { family: AggregateFamily },
    DuplicateEntry { family: AggregateFamily },
    DuplicateEventId,
    InvalidBucketShape { family: AggregateFamily },
    InvalidPublicRowSpend { family: AggregateFamily },
    InvalidSessionShape { family: AggregateFamily },
    TooManySessions { family: AggregateFamily },
    DuplicateSession { family: AggregateFamily },
    InvalidSessionRelationship { family: AggregateFamily },
    InvalidSessionContributionCount { family: AggregateFamily },
}

impl fmt::Display for AggregateStateInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntryIdentityMismatch { family } => {
                write!(
                    formatter,
                    "{family} aggregate entry has inconsistent identity"
                )
            }
            Self::DuplicateEntry { family } => {
                write!(
                    formatter,
                    "{family} aggregate state contains a duplicate key"
                )
            }
            Self::DuplicateEventId => {
                formatter.write_str("aggregate private state contains a duplicate event id")
            }
            Self::InvalidBucketShape { family } => {
                write!(
                    formatter,
                    "{family} aggregate bucket has inconsistent fields"
                )
            }
            Self::InvalidPublicRowSpend { family } => {
                write!(
                    formatter,
                    "{family} aggregate public-row spend is inconsistent"
                )
            }
            Self::InvalidSessionShape { family } => {
                write!(
                    formatter,
                    "{family} aggregate session fields are inconsistent"
                )
            }
            Self::TooManySessions { family } => {
                write!(
                    formatter,
                    "{family} aggregate session set exceeds its bound"
                )
            }
            Self::DuplicateSession { family } => {
                write!(
                    formatter,
                    "{family} aggregate session set contains a duplicate"
                )
            }
            Self::InvalidSessionRelationship { family } => {
                write!(
                    formatter,
                    "{family} aggregate session sets have an invalid relationship"
                )
            }
            Self::InvalidSessionContributionCount { family } => write!(
                formatter,
                "{family} aggregate session sets disagree with their contribution counts"
            ),
        }
    }
}

impl Error for AggregateStateInvariantError {}
