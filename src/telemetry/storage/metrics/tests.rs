use std::fs;

use chrono::NaiveDate;

use super::*;
use crate::telemetry::schema::recorded_data_example_row;

fn day(day: u32) -> UtcDay {
    UtcDay::from_date(NaiveDate::from_ymd_opt(2026, 8, day).unwrap())
}

fn aggregate(kind: &str) -> AggregateRow {
    aggregate_value(serde_json::from_str(recorded_data_example_row(kind)).unwrap())
}

fn aggregate_with_event_id(kind: &str, event_id: &str) -> AggregateRow {
    let mut value: serde_json::Value =
        serde_json::from_str(recorded_data_example_row(kind)).unwrap();
    value["event_id"] = serde_json::Value::String(event_id.to_owned());
    aggregate_value(value)
}

fn aggregate_value(value: serde_json::Value) -> AggregateRow {
    let json = serde_json::to_string(&value).unwrap();
    let RowClassification::Supported(TelemetryRow::Aggregate(row)) = classify_row(&json) else {
        panic!("test fixture did not produce a supported aggregate row")
    };
    row
}

fn canonical_line(row: &AggregateRow) -> Vec<u8> {
    let mut line = serde_json::to_vec(row).unwrap();
    line.push(b'\n');
    line
}

fn storage(temporary: &tempfile::TempDir) -> LockedStorage {
    LockedStorage::try_acquire(temporary.path()).unwrap()
}

fn write_metric_file(storage: &LockedStorage, day: UtcDay, bytes: &[u8]) {
    fs::write(storage.paths.metrics_file(day), bytes).unwrap();
}

fn prepared(day: UtcDay, byte_len: usize) -> PreparedMetricSnapshot {
    PreparedMetricSnapshot {
        day,
        bytes: vec![b'x'; byte_len].into_boxed_slice(),
    }
}

fn usage(
    event_bytes: u64,
    event_tail_needs_repair: bool,
    event_file_ends_with_marker: bool,
) -> MetricStorageUsage {
    MetricStorageUsage {
        event_bytes,
        event_tail_needs_repair,
        event_file_ends_with_marker,
    }
}

#[test]
fn missing_and_empty_metric_files_load_as_empty_snapshots() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);

    let missing = storage.load_metric_snapshot(day).unwrap();
    write_metric_file(&storage, day, b"");
    let empty = storage.load_metric_snapshot(day).unwrap();

    assert_eq!(missing, MetricSnapshot::empty(day));
    assert_eq!(empty, MetricSnapshot::empty(day));
}

#[test]
fn every_aggregate_kind_loads_and_prepares_in_canonical_kind_order() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let plugin = aggregate("plugin_hook_metrics");
    let hook = aggregate("hook_metrics");
    let extension = aggregate("extension_invocation_metrics");
    let mut bytes = canonical_line(&plugin);
    bytes.extend_from_slice(&canonical_line(&hook));
    bytes.extend_from_slice(&canonical_line(&extension));
    write_metric_file(&storage, day, &bytes);

    let snapshot = storage.load_metric_snapshot(day).unwrap();
    let prepared = snapshot.prepare().unwrap();
    let kinds: Vec<_> = prepared
        .bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap()["kind"].clone())
        .collect();

    assert_eq!(
        kinds,
        [
            "extension_invocation_metrics",
            "hook_metrics",
            "plugin_hook_metrics",
        ]
    );
}

#[test]
fn untouched_sibling_keeps_its_bytes_while_its_position_may_change() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let hook = aggregate("hook_metrics");
    let plugin = aggregate("plugin_hook_metrics");
    let mut unusual_hook_line = vec![b' ', b' '];
    unusual_hook_line.extend_from_slice(&serde_json::to_vec(&hook).unwrap());
    unusual_hook_line.extend_from_slice(b"\r\n");
    let mut bytes = canonical_line(&plugin);
    bytes.extend_from_slice(&unusual_hook_line);
    write_metric_file(&storage, day, &bytes);

    let mut snapshot = storage.load_metric_snapshot(day).unwrap();
    let plugin_id = plugin.event_id();
    snapshot.replace(plugin).unwrap();
    let prepared = snapshot.prepare().unwrap();

    assert!(
        prepared
            .bytes
            .windows(unusual_hook_line.len())
            .any(|window| window == unusual_hook_line)
    );
    assert_eq!(snapshot.row(plugin_id).unwrap().event_id(), plugin_id);
}

#[test]
fn insert_and_replace_require_explicit_event_id_state() {
    let day = day(3);
    let hook = aggregate("hook_metrics");
    let hook_id = hook.event_id();
    let plugin = aggregate("plugin_hook_metrics");
    let plugin_id = plugin.event_id();
    let mut snapshot = MetricSnapshot::empty(day);

    snapshot.insert(hook.clone()).unwrap();
    assert_eq!(
        snapshot.insert(hook.clone()),
        Err(MetricSnapshotEditError::DuplicateEventId(hook_id))
    );
    assert_eq!(
        snapshot.replace(plugin.clone()),
        Err(MetricSnapshotEditError::MissingEventId(plugin_id))
    );
    snapshot.replace(hook).unwrap();

    assert!(snapshot.row(hook_id).is_some());
}

#[test]
fn edits_reject_rows_from_another_day() {
    let mut value: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("hook_metrics")).unwrap();
    value["day"] = serde_json::Value::String("2026-08-04".to_owned());
    let row = aggregate_value(value);
    let mut snapshot = MetricSnapshot::empty(day(3));

    let result = snapshot.insert(row);

    assert_eq!(
        result,
        Err(MetricSnapshotEditError::DayMismatch {
            snapshot_day: day(3),
            row_day: day(4),
        })
    );
}

#[test]
fn duplicate_event_ids_fail_closed_but_duplicate_dimensions_do_not() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let first = aggregate("hook_metrics");
    let mut duplicate_id = canonical_line(&first);
    duplicate_id.extend_from_slice(&canonical_line(&first));
    write_metric_file(&storage, day, &duplicate_id);

    let error = storage.load_metric_snapshot(day).unwrap_err();

    assert!(matches!(
        error,
        LoadMetricSnapshotError::DuplicateEventId {
            first_line: 1,
            duplicate_line: 2,
        }
    ));

    let same_dimensions =
        aggregate_with_event_id("hook_metrics", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
    let mut legal = canonical_line(&first);
    legal.extend_from_slice(&canonical_line(&same_dimensions));
    write_metric_file(&storage, day, &legal);

    assert!(storage.load_metric_snapshot(day).is_ok());
}

#[test]
fn malformed_unknown_invalid_and_unexpected_rows_keep_distinct_categories() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);

    write_metric_file(&storage, day, b"{not json}\n");
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::MalformedLine { line: 1 })
    ));

    let mut unknown: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("hook_metrics")).unwrap();
    unknown["v"] = serde_json::Value::from(2);
    write_metric_file(&storage, day, &canonical_json_line(&unknown));
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::UnknownSchema { line: 1 })
    ));

    let mut invalid: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("hook_metrics")).unwrap();
    invalid.as_object_mut().unwrap().remove("invocations");
    write_metric_file(&storage, day, &canonical_json_line(&invalid));
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::InvalidRow { line: 1 })
    ));

    let low_volume = recorded_data_example_row("command");
    let mut line = low_volume.as_bytes().to_vec();
    line.push(b'\n');
    write_metric_file(&storage, day, &line);
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::UnexpectedRow {
            line: 1,
            reason: UnexpectedMetricRow::LowVolume,
        })
    ));
}

#[test]
fn wrong_day_and_unterminated_rows_fail_closed() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let mut other_day: serde_json::Value =
        serde_json::from_str(recorded_data_example_row("hook_metrics")).unwrap();
    other_day["day"] = serde_json::Value::String("2026-08-04".to_owned());
    write_metric_file(&storage, day, &canonical_json_line(&other_day));
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::UnexpectedRow {
            line: 1,
            reason: UnexpectedMetricRow::DayMismatch,
        })
    ));

    let unterminated = serde_json::to_vec(&aggregate("hook_metrics")).unwrap();
    write_metric_file(&storage, day, &unterminated);
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::UnterminatedFinalLine { line: 1 })
    ));
}

#[test]
fn content_errors_never_retain_or_print_the_line() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let sensitive = "recognisable-sensitive-value";
    let line = format!(r#"{{"{sensitive}":"unterminated}}"#);
    write_metric_file(&storage, day(3), format!("{line}\n").as_bytes());

    let error = storage.load_metric_snapshot(day(3)).unwrap_err();

    assert!(!error.to_string().contains(sensitive));
    assert!(!format!("{error:?}").contains(sensitive));
    assert!(error.source().is_none());
}

#[test]
fn loader_accepts_the_exact_cap_and_rejects_one_byte_more() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let line = canonical_line(&aggregate("hook_metrics"));
    let mut exact = vec![b' '; MAX_METRIC_SNAPSHOT_BYTES - line.len()];
    exact.extend_from_slice(&line);
    write_metric_file(&storage, day, &exact);

    assert!(storage.load_metric_snapshot(day).is_ok());

    exact.insert(0, b' ');
    write_metric_file(&storage, day, &exact);
    assert!(matches!(
        storage.load_metric_snapshot(day),
        Err(LoadMetricSnapshotError::TooLarge {
            maximum: MAX_METRIC_SNAPSHOT_BYTES,
        })
    ));
}

#[test]
fn snapshot_growing_past_the_cap_is_suppressed_without_changing_the_file() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    let line = canonical_line(&aggregate("hook_metrics"));
    let mut exact = vec![b' '; MAX_METRIC_SNAPSHOT_BYTES - line.len()];
    exact.extend_from_slice(&line);
    write_metric_file(&storage, day, &exact);
    let original = fs::read(storage.paths.metrics_file(day)).unwrap();
    let mut snapshot = storage.load_metric_snapshot(day).unwrap();
    snapshot.insert(aggregate("plugin_hook_metrics")).unwrap();
    let prepared = snapshot.prepare().unwrap();

    let decision = storage.plan_metric_write(prepared, Some(day));

    assert_eq!(
        decision,
        MetricWrite::Suppressed(MetricSuppression::SnapshotTooLarge)
    );
    assert_eq!(fs::read(storage.paths.metrics_file(day)).unwrap(), original);
}

#[test]
fn daily_allowance_accounts_for_event_bytes_repair_marker_and_snapshot() {
    let day = day(3);
    let usage = usage(10, true, false);
    let exact = select_metric_write(36, usage, 5, None, prepared(day, 20));
    let over = select_metric_write(35, usage, 5, None, prepared(day, 20));

    assert!(matches!(exact, MetricWrite::Replace(_)));
    assert_eq!(
        over,
        MetricWrite::Suppressed(MetricSuppression::DailyAllowanceExceeded)
    );
}

#[test]
fn state_or_final_marker_closes_the_event_reservation() {
    let day = day(3);
    let open_usage = usage(10, true, false);
    let marker_usage = usage(10, true, true);

    let state_closed = select_metric_write(30, open_usage, 100, Some(day), prepared(day, 20));
    let marker_closed = select_metric_write(30, marker_usage, 100, None, prepared(day, 20));

    assert!(matches!(state_closed, MetricWrite::Replace(_)));
    assert!(matches!(marker_closed, MetricWrite::Replace(_)));
}

#[test]
fn usage_inspection_failure_suppresses_only_the_metric_replacement() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    fs::create_dir(storage.paths.event_file(day)).unwrap();

    let decision = storage.plan_metric_write(prepared(day, 1), None);

    assert_eq!(
        decision,
        MetricWrite::Suppressed(MetricSuppression::UsageUnavailable)
    );
}

#[test]
fn oversized_snapshot_is_rejected_before_filesystem_inspection() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = storage(&temporary);
    let day = day(3);
    fs::create_dir(storage.paths.event_file(day)).unwrap();

    let decision = storage.plan_metric_write(prepared(day, MAX_METRIC_SNAPSHOT_BYTES + 1), None);

    assert_eq!(
        decision,
        MetricWrite::Suppressed(MetricSuppression::SnapshotTooLarge)
    );
}

#[test]
fn atomic_replacement_writes_only_the_prepared_snapshot() {
    let temporary = tempfile::tempdir().unwrap();
    let mut storage = storage(&temporary);
    let day = day(3);
    let mut snapshot = MetricSnapshot::empty(day);
    snapshot.insert(aggregate("hook_metrics")).unwrap();
    let prepared = snapshot.prepare().unwrap();

    storage.replace_metric_snapshot(&prepared).unwrap();

    assert_eq!(
        fs::read(storage.paths.metrics_file(day)).unwrap(),
        prepared.bytes.as_ref()
    );
}

#[cfg(unix)]
#[test]
fn replaced_metric_snapshot_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let temporary = tempfile::tempdir().unwrap();
    let mut storage = storage(&temporary);
    let day = day(3);
    let prepared = prepared(day, 1);

    storage.replace_metric_snapshot(&prepared).unwrap();

    let mode = fs::metadata(storage.paths.metrics_file(day))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

fn canonical_json_line(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}
