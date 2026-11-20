//! A new batch's name: when it started, in UTC, so batches sort by time, and a few
//! random digits, so two started in the same second are unlikely to collide. A collision
//! is still safe: [`crate::run::Batch::create`] refuses to reuse an existing batch.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::task::BatchName;

/// A name such as `20261106T183005Z-a3f9`.
pub(crate) fn new_batch_name(now: DateTime<Utc>) -> BatchName {
    let random = Uuid::new_v4().simple().to_string();
    let name = format!("{}-{}", now.format("%Y%m%dT%H%M%SZ"), &random[..4]);
    BatchName::new(name).expect("a timestamp and hex digits are a portable name")
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn a_batch_name_contains_its_start_time_and_a_random_suffix() {
        let now = Utc.with_ymd_and_hms(2026, 11, 6, 18, 30, 5).unwrap();

        let name = new_batch_name(now);

        let (time, random) = name.as_str().split_once('-').unwrap();
        assert_eq!(time, "20261106T183005Z");
        assert_eq!(random.len(), 4);
        assert!(
            random.chars().all(|digit| digit.is_ascii_hexdigit()),
            "{name}"
        );
    }
}
