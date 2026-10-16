//! Bounded, fail-closed aggregate metric snapshots.
//!
//! A snapshot is replaced atomically, so an unterminated final line is not
//! repaired like an append-only event file. It means the file was corrupted
//! or changed outside this writer, and aggregate recording fails closed for
//! that day. Low-volume event appends remain independent.
//!
//! Loaded rows retain their complete physical line. Replacing one row
//! canonically serializes only that row; untouched siblings keep their bytes
//! even when canonical sorting moves their position. A total wire-label key,
//! ending in `event_id`, removes arrival order from the metric file.
//!
//! These crate-internal publication primitives temporarily rely on callers to
//! select policy before writing. The transaction layer will carry selected
//! event and metric bytes inside `PersistedRecording`, making publication
//! require proof that their private state was replaced successfully.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    fs::File,
    io::{self, Read as _},
};

use crate::telemetry::schema::{
    AggregateRow, EventId, RowClassification, TelemetryRow, UtcDay, classify_row,
};

use super::{
    LockedStorage,
    atomic::{self, AtomicReplaceError},
    limits::{
        DAILY_STORAGE_ALLOWANCE_BYTES, EventFileUsage, inspect_event_file,
        reserved_storage_limit_line_bytes,
    },
};

/// Maximum physical size of one aggregate snapshot.
pub(super) const MAX_METRIC_SNAPSHOT_BYTES: usize = 512 * 1024;

/// One validated aggregate row and the physical line loaded for it.
#[derive(Clone, PartialEq, Eq)]
struct StoredAggregateRow {
    row: AggregateRow,
    original_line: Option<Box<[u8]>>,
}

/// Complete in-memory aggregate snapshot for one UTC day.
#[derive(Clone, PartialEq, Eq)]
pub(in crate::telemetry) struct MetricSnapshot {
    day: UtcDay,
    rows: Vec<StoredAggregateRow>,
}

impl fmt::Debug for MetricSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MetricSnapshot")
            .field("day", &self.day)
            .field("row_count", &self.rows.len())
            .finish()
    }
}

impl MetricSnapshot {
    /// Create an empty snapshot owned by `day`.
    #[must_use]
    pub(in crate::telemetry) const fn empty(day: UtcDay) -> Self {
        Self {
            day,
            rows: Vec::new(),
        }
    }

    /// Borrow the row with this exact persisted identifier.
    #[must_use]
    pub(in crate::telemetry) fn row(&self, event_id: EventId) -> Option<&AggregateRow> {
        self.rows
            .iter()
            .find(|stored| stored.row.event_id() == event_id)
            .map(|stored| &stored.row)
    }

    /// Add a new row without silently replacing an existing identifier.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the row belongs to another day or its event
    /// identifier is already present.
    pub(in crate::telemetry) fn insert(
        &mut self,
        row: AggregateRow,
    ) -> Result<(), MetricSnapshotEditError> {
        self.ensure_day(&row)?;
        let event_id = row.event_id();
        if self.row(event_id).is_some() {
            return Err(MetricSnapshotEditError::DuplicateEventId(event_id));
        }
        self.rows.push(StoredAggregateRow {
            row,
            original_line: None,
        });
        Ok(())
    }

    /// Replace exactly one existing row and mark only it for serialization.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the row belongs to another day or no row has
    /// its event identifier.
    pub(in crate::telemetry) fn replace(
        &mut self,
        row: AggregateRow,
    ) -> Result<(), MetricSnapshotEditError> {
        self.ensure_day(&row)?;
        let event_id = row.event_id();
        let Some(stored) = self
            .rows
            .iter_mut()
            .find(|stored| stored.row.event_id() == event_id)
        else {
            return Err(MetricSnapshotEditError::MissingEventId(event_id));
        };

        stored.row = row;
        stored.original_line = None;
        Ok(())
    }

    /// Canonically order rows and fully prepare the replacement bytes.
    ///
    /// # Errors
    ///
    /// Returns the defensive JSON serialization error for a newly inserted or
    /// replaced typed row. Existing physical lines require no serialization.
    pub(in crate::telemetry) fn prepare(
        &self,
    ) -> Result<PreparedMetricSnapshot, PrepareMetricSnapshotError> {
        // The key borrows public names from each row, so sort references while
        // leaving the owning snapshot in place.
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_unstable_by_key(|stored| stored.row.sort_key());

        let mut bytes = Vec::new();
        for stored in rows {
            if let Some(line) = &stored.original_line {
                bytes.extend_from_slice(line);
            } else {
                serde_json::to_writer(&mut bytes, &stored.row)
                    .map_err(PrepareMetricSnapshotError::Serialize)?;
                bytes.push(b'\n');
            }
        }

        Ok(PreparedMetricSnapshot {
            day: self.day,
            bytes: bytes.into_boxed_slice(),
        })
    }

    fn ensure_day(&self, row: &AggregateRow) -> Result<(), MetricSnapshotEditError> {
        if row.day() == self.day {
            Ok(())
        } else {
            Err(MetricSnapshotEditError::DayMismatch {
                snapshot_day: self.day,
                row_day: row.day(),
            })
        }
    }
}

/// Invalid explicit edit to an in-memory metric snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum MetricSnapshotEditError {
    DayMismatch {
        snapshot_day: UtcDay,
        row_day: UtcDay,
    },
    DuplicateEventId(EventId),
    MissingEventId(EventId),
}

impl fmt::Display for MetricSnapshotEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayMismatch {
                snapshot_day,
                row_day,
            } => write!(
                formatter,
                "metric snapshot belongs to {snapshot_day}, not row day {row_day}"
            ),
            Self::DuplicateEventId(event_id) => {
                write!(formatter, "metric snapshot already contains row {event_id}")
            }
            Self::MissingEventId(event_id) => {
                write!(formatter, "metric snapshot contains no row {event_id}")
            }
        }
    }
}

impl Error for MetricSnapshotEditError {}

/// Complete bytes ready for one atomic metric-snapshot replacement.
#[derive(PartialEq, Eq)]
#[must_use]
pub(in crate::telemetry) struct PreparedMetricSnapshot {
    day: UtcDay,
    bytes: Box<[u8]>,
}

impl PreparedMetricSnapshot {
    /// Return the day selected by the replacement path.
    #[must_use]
    pub(in crate::telemetry) const fn day(&self) -> UtcDay {
        self.day
    }

    /// Return the complete replacement byte length.
    #[must_use]
    pub(in crate::telemetry) const fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

impl fmt::Debug for PreparedMetricSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedMetricSnapshot")
            .field("day", &self.day)
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

/// Defensive serialization failure while preparing a typed snapshot.
#[derive(Debug)]
pub(in crate::telemetry) enum PrepareMetricSnapshotError {
    Serialize(serde_json::Error),
}

impl fmt::Display for PrepareMetricSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("failed to serialize aggregate metric row")
    }
}

impl Error for PrepareMetricSnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialize(error) => Some(error),
        }
    }
}

/// Recognized row that cannot belong to an aggregate snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum UnexpectedMetricRow {
    LowVolume,
    DayMismatch,
}

/// Failure to load one bounded daily metric snapshot.
#[derive(Debug)]
pub(in crate::telemetry) enum LoadMetricSnapshotError {
    Io(io::Error),
    TooLarge {
        maximum: usize,
    },
    UnterminatedFinalLine {
        line: usize,
    },
    MalformedLine {
        line: usize,
    },
    UnknownSchema {
        line: usize,
    },
    InvalidRow {
        line: usize,
    },
    UnexpectedRow {
        line: usize,
        reason: UnexpectedMetricRow,
    },
    DuplicateEventId {
        first_line: usize,
        duplicate_line: usize,
    },
}

impl fmt::Display for LoadMetricSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("failed to read aggregate metric snapshot"),
            Self::TooLarge { maximum } => write!(
                formatter,
                "aggregate metric snapshot exceeds the {maximum}-byte safety limit"
            ),
            Self::UnterminatedFinalLine { line } => {
                write!(
                    formatter,
                    "aggregate metric snapshot line {line} is unterminated"
                )
            }
            Self::MalformedLine { line } => {
                write!(
                    formatter,
                    "aggregate metric snapshot line {line} is malformed"
                )
            }
            Self::UnknownSchema { line } => write!(
                formatter,
                "aggregate metric snapshot line {line} has an unknown schema"
            ),
            Self::InvalidRow { line } => write!(
                formatter,
                "aggregate metric snapshot line {line} violates its known schema"
            ),
            Self::UnexpectedRow { line, reason } => match reason {
                UnexpectedMetricRow::LowVolume => write!(
                    formatter,
                    "aggregate metric snapshot line {line} is a low-volume row"
                ),
                UnexpectedMetricRow::DayMismatch => write!(
                    formatter,
                    "aggregate metric snapshot line {line} belongs to another day"
                ),
            },
            Self::DuplicateEventId {
                first_line,
                duplicate_line,
            } => write!(
                formatter,
                "aggregate metric snapshot lines {first_line} and {duplicate_line} repeat one event id"
            ),
        }
    }
}

impl Error for LoadMetricSnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::TooLarge { .. }
            | Self::UnterminatedFinalLine { .. }
            | Self::MalformedLine { .. }
            | Self::UnknownSchema { .. }
            | Self::InvalidRow { .. }
            | Self::UnexpectedRow { .. }
            | Self::DuplicateEventId { .. } => None,
        }
    }
}

impl From<io::Error> for LoadMetricSnapshotError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Selected action for the aggregate metric file.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub(in crate::telemetry) enum MetricWrite {
    Replace(PreparedMetricSnapshot),
    Suppressed(MetricSuppression),
}

/// Reason an otherwise valid aggregate snapshot must not be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum MetricSuppression {
    SnapshotTooLarge,
    DailyAllowanceExceeded,
    UsageUnavailable,
}

/// Failure to atomically replace the metric snapshot.
#[derive(Debug)]
pub(in crate::telemetry) struct ReplaceMetricSnapshotError(AtomicReplaceError);

impl fmt::Display for ReplaceMetricSnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("failed to replace aggregate metric snapshot")
    }
}

impl Error for ReplaceMetricSnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

impl LockedStorage {
    /// Load one complete aggregate snapshot without discarding unknown bytes.
    ///
    /// Missing and empty files produce an empty snapshot. Every non-empty
    /// physical line must be a valid aggregate row for `day` and must end in a
    /// line feed.
    ///
    /// # Errors
    ///
    /// Returns a content-free line category, a bounded-size error, or an I/O
    /// error. Unknown, invalid, and malformed files remain untouched.
    pub(in crate::telemetry) fn load_metric_snapshot(
        &self,
        day: UtcDay,
    ) -> Result<MetricSnapshot, LoadMetricSnapshotError> {
        let path = self.paths.metrics_file(day);
        let Some(bytes) = read_bounded_snapshot(&path)? else {
            return Ok(MetricSnapshot::empty(day));
        };
        if bytes.is_empty() {
            return Ok(MetricSnapshot::empty(day));
        }
        if bytes.last() != Some(&b'\n') {
            return Err(LoadMetricSnapshotError::UnterminatedFinalLine {
                line: bytes.iter().filter(|byte| **byte == b'\n').count() + 1,
            });
        }

        let mut snapshot = MetricSnapshot::empty(day);
        let mut first_line_by_event = BTreeMap::new();
        for (index, physical_line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
            let line_number = index + 1;
            let json = std::str::from_utf8(&physical_line[..physical_line.len() - 1])
                .map_err(|_| LoadMetricSnapshotError::MalformedLine { line: line_number })?;
            let row = match classify_row(json) {
                RowClassification::Supported(TelemetryRow::Aggregate(row)) => row,
                RowClassification::Supported(TelemetryRow::LowVolume(_)) => {
                    return Err(LoadMetricSnapshotError::UnexpectedRow {
                        line: line_number,
                        reason: UnexpectedMetricRow::LowVolume,
                    });
                }
                RowClassification::UnknownSchema => {
                    return Err(LoadMetricSnapshotError::UnknownSchema { line: line_number });
                }
                RowClassification::Invalid => {
                    return Err(LoadMetricSnapshotError::InvalidRow { line: line_number });
                }
                RowClassification::Malformed => {
                    return Err(LoadMetricSnapshotError::MalformedLine { line: line_number });
                }
            };
            if row.day() != day {
                return Err(LoadMetricSnapshotError::UnexpectedRow {
                    line: line_number,
                    reason: UnexpectedMetricRow::DayMismatch,
                });
            }
            if let Some(first_line) = first_line_by_event.insert(row.event_id(), line_number) {
                return Err(LoadMetricSnapshotError::DuplicateEventId {
                    first_line,
                    duplicate_line: line_number,
                });
            }
            snapshot.rows.push(StoredAggregateRow {
                row,
                original_line: Some(Box::from(physical_line)),
            });
        }

        Ok(snapshot)
    }

    /// Select whether a fully prepared snapshot fits both storage ceilings.
    ///
    /// Event bytes are measured after the event side has published. A closed
    /// event day needs neither a repair byte nor a future marker reservation.
    pub(in crate::telemetry) fn plan_metric_write(
        &self,
        prepared: PreparedMetricSnapshot,
        stopped_day: Option<UtcDay>,
    ) -> MetricWrite {
        if prepared.byte_len() > MAX_METRIC_SNAPSHOT_BYTES {
            return MetricWrite::Suppressed(MetricSuppression::SnapshotTooLarge);
        }

        let day = prepared.day();
        let event_usage = match inspect_event_file(&self.paths.event_file(day), day) {
            Ok(usage) => usage,
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "telemetry: event usage unavailable; suppressing metric replacement"
                );
                return MetricWrite::Suppressed(MetricSuppression::UsageUnavailable);
            }
        };

        select_metric_write(
            DAILY_STORAGE_ALLOWANCE_BYTES,
            event_usage.into(),
            reserved_storage_limit_line_bytes(day),
            stopped_day,
            prepared,
        )
    }

    /// Atomically replace one fully prepared aggregate snapshot.
    ///
    /// # Errors
    ///
    /// Returns the atomic replacement failure. Serialization and size policy
    /// have already completed before this method creates a temporary file.
    pub(in crate::telemetry) fn replace_metric_snapshot(
        &mut self,
        prepared: &PreparedMetricSnapshot,
    ) -> Result<(), ReplaceMetricSnapshotError> {
        atomic::replace(&self.paths.metrics_file(prepared.day), &prepared.bytes)
            .map_err(ReplaceMetricSnapshotError)
    }
}

/// Event-file facts consumed by the pure metric-size policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetricStorageUsage {
    event_bytes: u64,
    event_tail_needs_repair: bool,
    event_file_ends_with_marker: bool,
}

impl From<EventFileUsage> for MetricStorageUsage {
    fn from(usage: EventFileUsage) -> Self {
        Self {
            event_bytes: usage.byte_len(),
            event_tail_needs_repair: usage.tail_needs_repair(),
            event_file_ends_with_marker: usage.ends_with_marker(),
        }
    }
}

/// Make the snapshot-size decision without filesystem access.
fn select_metric_write(
    allowance: u64,
    usage: MetricStorageUsage,
    marker_reservation: u64,
    stopped_day: Option<UtcDay>,
    prepared: PreparedMetricSnapshot,
) -> MetricWrite {
    if prepared.byte_len() > MAX_METRIC_SNAPSHOT_BYTES {
        return MetricWrite::Suppressed(MetricSuppression::SnapshotTooLarge);
    }

    let event_day_is_closed =
        stopped_day == Some(prepared.day()) || usage.event_file_ends_with_marker;
    let repair_bytes = if event_day_is_closed {
        0
    } else {
        u64::from(usage.event_tail_needs_repair)
    };
    let marker_bytes = if event_day_is_closed {
        0
    } else {
        marker_reservation
    };
    let snapshot_bytes = u64::try_from(prepared.byte_len()).unwrap_or(u64::MAX);
    let total = usage
        .event_bytes
        .saturating_add(repair_bytes)
        .saturating_add(marker_bytes)
        .saturating_add(snapshot_bytes);

    if total <= allowance {
        MetricWrite::Replace(prepared)
    } else {
        MetricWrite::Suppressed(MetricSuppression::DailyAllowanceExceeded)
    }
}

fn read_bounded_snapshot(
    path: &std::path::Path,
) -> Result<Option<Vec<u8>>, LoadMetricSnapshotError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let limit = u64::try_from(MAX_METRIC_SNAPSHOT_BYTES + 1)
        .expect("BUG: metric-snapshot read limit must fit in u64");
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_METRIC_SNAPSHOT_BYTES {
        return Err(LoadMetricSnapshotError::TooLarge {
            maximum: MAX_METRIC_SNAPSHOT_BYTES,
        });
    }

    Ok(Some(bytes))
}

#[cfg(test)]
mod tests;
