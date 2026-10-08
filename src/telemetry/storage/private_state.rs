//! Bounded, privacy-preserving I/O for telemetry-state.toml.

use std::{
    error::Error,
    fmt,
    fs::File,
    io::{self, Read as _},
    path::Path,
};

use super::{
    LockedStorage,
    atomic::{self, AtomicReplaceError},
    daily_files,
};
use crate::telemetry::schema::UtcDay;
use crate::telemetry::state::{
    StateContentError, StateDecodeError, TelemetryStateV1, decode, encode,
};

/// Safety ceiling for private state read into one recorder process.
///
/// The aggregate-backed state shape will get a generated worst-case benchmark
/// before recording is activated. This limit gives the current estimate more
/// than twice its expected headroom while bounding corrupt input now.
const MAX_PRIVATE_STATE_BYTES: usize = 16 * 1024 * 1024;

/// Failure to load private telemetry state while holding its lock.
#[derive(Debug)]
pub(in crate::telemetry) enum LoadStateError {
    Io(io::Error),
    TooLarge { maximum: usize },
    Content(StateContentError),
    UnsupportedVersion(u64),
}

impl fmt::Display for LoadStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(_) => formatter.write_str("failed to read telemetry private state"),
            Self::TooLarge { maximum } => write!(
                formatter,
                "telemetry private state exceeds the {maximum}-byte safety limit"
            ),
            Self::Content(error) => error.fmt(formatter),
            Self::UnsupportedVersion(version) => {
                StateDecodeError::UnsupportedVersion(*version).fmt(formatter)
            }
        }
    }
}

impl Error for LoadStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::TooLarge { .. } | Self::Content(_) | Self::UnsupportedVersion(_) => None,
        }
    }
}

impl From<io::Error> for LoadStateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StateDecodeError> for LoadStateError {
    fn from(error: StateDecodeError) -> Self {
        match error {
            StateDecodeError::Malformed(error) => Self::Content(error),
            StateDecodeError::UnsupportedVersion(version) => Self::UnsupportedVersion(version),
        }
    }
}

/// Failure to load existing state or initialize its first identity epoch.
#[derive(Debug)]
pub(in crate::telemetry) enum OpenStateError {
    Load(LoadStateError),
    InspectDailyFiles(io::Error),
    GenerateIdentityKey(getrandom::Error),
}

impl fmt::Display for OpenStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(_) => formatter.write_str("failed to load existing telemetry private state"),
            Self::InspectDailyFiles(_) => {
                formatter.write_str("failed to inspect existing telemetry day files")
            }
            Self::GenerateIdentityKey(_) => {
                formatter.write_str("failed to generate a telemetry identity key")
            }
        }
    }
}

impl Error for OpenStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::InspectDailyFiles(error) => Some(error),
            Self::GenerateIdentityKey(error) => Some(error),
        }
    }
}

impl From<LoadStateError> for OpenStateError {
    fn from(error: LoadStateError) -> Self {
        Self::Load(error)
    }
}

/// Failure to serialize or atomically replace private telemetry state.
#[derive(Debug)]
pub(in crate::telemetry) enum ReplaceStateError {
    Serialize(toml::ser::Error),
    Replace(AtomicReplaceError),
}

impl fmt::Display for ReplaceStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(_) => {
                formatter.write_str("failed to serialize telemetry private state")
            }
            Self::Replace(_) => formatter.write_str("failed to replace telemetry private state"),
        }
    }
}

impl Error for ReplaceStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialize(error) => Some(error),
            Self::Replace(error) => Some(error),
        }
    }
}

impl LockedStorage {
    /// Load and validate private state without retaining sensitive diagnostics.
    ///
    /// # Errors
    ///
    /// Returns an I/O error, a sanitized malformed-state reason, or the
    /// unsupported version found in an otherwise syntactically valid document.
    pub(in crate::telemetry) fn load_state(
        &self,
    ) -> Result<Option<TelemetryStateV1>, LoadStateError> {
        let Some(bytes) = read_bounded(self.paths.state_file())? else {
            return Ok(None);
        };

        decode(&bytes).map(Some).map_err(Into::into)
    }

    /// Load private state, or initialize it at the newest day that may exist.
    ///
    /// Missing state is initialized in memory so the recorder can apply its
    /// complete operation and persist once. Existing malformed or unsupported
    /// state is never replaced here. A surviving daily file dated after
    /// `current_day` intentionally initializes a future high-water mark, so
    /// recording remains paused until the clock catches up.
    ///
    /// # Errors
    ///
    /// Returns a load error for existing state, an inspection error when the
    /// newest surviving daily file cannot be determined, or a key-generation
    /// error for first use.
    pub(in crate::telemetry) fn load_or_initialize_state(
        &self,
        current_day: UtcDay,
    ) -> Result<TelemetryStateV1, OpenStateError> {
        if let Some(state) = self.load_state()? {
            return Ok(state);
        }

        let initial_day = daily_files::newest_day(self.paths.telemetry_dir())
            .map_err(OpenStateError::InspectDailyFiles)?
            .map_or(current_day, |newest| current_day.max(newest));
        TelemetryStateV1::new(initial_day).map_err(OpenStateError::GenerateIdentityKey)
    }

    /// Serialize completely, then atomically replace private state.
    ///
    /// Mutable access keeps two replacements from overlapping under one held
    /// lock. The replacement is atomic but intentionally not crash-durable.
    ///
    /// # Errors
    ///
    /// Returns the serialization or atomic replacement stage that failed.
    pub(in crate::telemetry) fn replace_state(
        &mut self,
        state: &TelemetryStateV1,
    ) -> Result<(), ReplaceStateError> {
        let serialized = encode(state).map_err(ReplaceStateError::Serialize)?;
        atomic::replace(self.paths.state_file(), serialized.as_bytes())
            .map_err(ReplaceStateError::Replace)
    }
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, LoadStateError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let limit = u64::try_from(MAX_PRIVATE_STATE_BYTES + 1)
        .expect("BUG: private-state read limit must fit in u64");
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PRIVATE_STATE_BYTES {
        return Err(LoadStateError::TooLarge {
            maximum: MAX_PRIVATE_STATE_BYTES,
        });
    }

    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use std::{error::Error as _, fs};

    use chrono::{NaiveDate, TimeZone as _, Utc};

    use super::{LoadStateError, MAX_PRIVATE_STATE_BYTES, OpenStateError};
    use crate::telemetry::{
        schema::{UtcDay, UtcSecond},
        state::{
            IDENTIFIER_WINDOW_TEST_STATE, RecordingObservationError, StateContentError,
            TelemetryStateV1, encode,
        },
        storage::LockedStorage,
    };

    fn storage(temporary: &tempfile::TempDir) -> LockedStorage {
        LockedStorage::try_acquire(temporary.path()).unwrap()
    }

    fn day(year: i32, month: u32, day: u32) -> UtcDay {
        UtcDay::from_date(NaiveDate::from_ymd_opt(year, month, day).unwrap())
    }

    fn assert_diagnostic_redacts(error: &LoadStateError, sensitive: &str) {
        assert!(!error.to_string().contains(sensitive));
        assert!(!format!("{error:?}").contains(sensitive));
        assert!(error.source().is_none());
    }

    #[test]
    fn missing_private_state_is_not_created_or_reported_as_an_error() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);

        let state = storage.load_state().unwrap();

        assert!(state.is_none());
        assert!(!storage.paths.state_file().exists());
    }

    #[test]
    fn first_state_uses_the_current_day_when_surviving_files_are_older() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        let current_day = day(2026, 9, 23);
        fs::write(
            storage
                .paths
                .telemetry_dir()
                .join("events-2026-08-03.jsonl"),
            [],
        )
        .unwrap();

        let state = storage.load_or_initialize_state(current_day).unwrap();
        let encoded = encode(&state).unwrap();

        assert_eq!(state.latest_opened_day(), current_day);
        assert!(encoded.contains("identifier-window-anchor = \"2026-09-23\""));
        assert!(encoded.contains("latest-opened-day = \"2026-09-23\""));
        assert!(!storage.paths.state_file().exists());
    }

    #[test]
    fn first_state_does_not_reopen_a_future_dated_daily_file() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::write(
            storage
                .paths
                .telemetry_dir()
                .join("events-2026-10-04.jsonl"),
            [],
        )
        .unwrap();
        fs::write(
            storage
                .paths
                .telemetry_dir()
                .join("metrics-2026-10-07.jsonl"),
            [],
        )
        .unwrap();

        let state = storage.load_or_initialize_state(day(2026, 9, 23)).unwrap();
        let encoded = encode(&state).unwrap();

        assert_eq!(state.latest_opened_day(), day(2026, 10, 7));
        assert!(encoded.contains("identifier-window-anchor = \"2026-10-07\""));
        assert!(encoded.contains("latest-opened-day = \"2026-10-07\""));
    }

    #[test]
    fn future_dated_recovery_pauses_recording_until_the_clock_catches_up() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::write(
            storage
                .paths
                .telemetry_dir()
                .join("events-2026-10-07.jsonl"),
            [],
        )
        .unwrap();
        let mut state = storage.load_or_initialize_state(day(2026, 9, 23)).unwrap();
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 9, 23, 12, 0, 0).unwrap());

        let result = state.observe_recording(completed_at);

        assert!(matches!(
            result,
            Err(RecordingObservationError::BeforeHighWater { .. })
        ));
        assert_eq!(state.latest_opened_day(), day(2026, 10, 7));
    }

    #[test]
    fn existing_unsupported_state_is_not_reinitialized() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        let source = b"version = 2\nfuture-field = true\n";
        fs::write(storage.paths.state_file(), source).unwrap();

        let Err(error) = storage.load_or_initialize_state(day(2026, 9, 23)) else {
            panic!("reinitialized private state with an unsupported version");
        };

        assert!(matches!(
            error,
            OpenStateError::Load(LoadStateError::UnsupportedVersion(2))
        ));
        assert_eq!(fs::read(storage.paths.state_file()).unwrap(), source);
    }

    #[test]
    fn existing_malformed_state_is_not_reinitialized() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        let source = concat!(
            "version = 1\n\n",
            "[identity]\n",
            "key = \"not-a-key\"\n",
            "identifier-window-anchor = \"2026-08-03\"\n",
            "latest-opened-day = \"2026-08-03\"\n",
        )
        .as_bytes();
        fs::write(storage.paths.state_file(), source).unwrap();

        let Err(error) = storage.load_or_initialize_state(day(2026, 9, 23)) else {
            panic!("reinitialized malformed private state");
        };

        assert!(matches!(
            error,
            OpenStateError::Load(LoadStateError::Content(
                StateContentError::InvalidState { .. }
            ))
        ));
        assert_eq!(fs::read(storage.paths.state_file()).unwrap(), source);
    }

    #[test]
    fn invalid_utf8_is_malformed_instead_of_an_io_failure() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::write(storage.paths.state_file(), b"version = 1\n\xff").unwrap();

        let Err(error) = storage.load_state() else {
            panic!("accepted private state containing invalid UTF-8");
        };

        assert!(matches!(
            error,
            LoadStateError::Content(StateContentError::InvalidUtf8 { .. })
        ));
    }

    #[test]
    fn malformed_state_diagnostics_never_retain_the_identity_key() {
        const RECOGNIZABLE_SECRET: &str = "recognizable-private-identity-key";

        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        let source = format!("version = 1\n\n[identity]\nkey = \"{RECOGNIZABLE_SECRET}\n");
        fs::write(storage.paths.state_file(), source).unwrap();

        let Err(error) = storage.load_state() else {
            panic!("accepted malformed private state");
        };
        assert!(matches!(
            &error,
            LoadStateError::Content(StateContentError::InvalidToml {
                line: Some(_),
                column: Some(_)
            })
        ));
        assert_diagnostic_redacts(&error, RECOGNIZABLE_SECRET);
    }

    #[test]
    fn invalid_state_diagnostics_never_retain_the_identity_key() {
        const RECOGNIZABLE_INVALID_KEY: &str = "recognizable-invalid-private-key";

        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        let source = format!(
            "version = 1\n\n[identity]\nkey = \"{RECOGNIZABLE_INVALID_KEY}\"\n\
             identifier-window-anchor = \"2026-08-03\"\n\
             latest-opened-day = \"2026-08-03\"\n"
        );
        fs::write(storage.paths.state_file(), source).unwrap();

        let Err(error) = storage.load_state() else {
            panic!("accepted private state containing an invalid identity key");
        };

        assert!(matches!(
            &error,
            LoadStateError::Content(StateContentError::InvalidState {
                line: Some(_),
                column: Some(_)
            })
        ));
        assert_diagnostic_redacts(&error, RECOGNIZABLE_INVALID_KEY);
    }

    #[test]
    fn unsupported_version_is_distinct_from_malformed_state() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::write(
            storage.paths.state_file(),
            b"version = 2\nfuture-field = true\n",
        )
        .unwrap();

        let Err(error) = storage.load_state() else {
            panic!("accepted an unsupported private-state version");
        };

        assert!(matches!(error, LoadStateError::UnsupportedVersion(2)));
    }

    #[test]
    fn private_state_read_failure_is_reported_as_io() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::create_dir(storage.paths.state_file()).unwrap();

        let Err(error) = storage.load_state() else {
            panic!("loaded a directory as private state");
        };

        assert!(matches!(error, LoadStateError::Io(_)));
    }

    #[test]
    fn oversized_private_state_is_rejected_at_the_read_boundary() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = storage(&temporary);
        fs::write(
            storage.paths.state_file(),
            vec![b' '; MAX_PRIVATE_STATE_BYTES + 1],
        )
        .unwrap();

        let Err(error) = storage.load_state() else {
            panic!("accepted oversized private state");
        };

        assert!(matches!(
            error,
            LoadStateError::TooLarge {
                maximum: MAX_PRIVATE_STATE_BYTES
            }
        ));
    }

    #[test]
    fn replacement_load_and_reserialization_preserve_canonical_bytes() {
        let temporary = tempfile::tempdir().unwrap();
        let mut storage = storage(&temporary);
        let state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();

        storage.replace_state(&state).unwrap();
        let on_disk = fs::read(storage.paths.state_file()).unwrap();
        let loaded = storage
            .load_state()
            .unwrap()
            .expect("the replaced state file must exist");
        let reserialized = toml::to_string_pretty(&loaded).unwrap().into_bytes();

        assert_eq!(on_disk, IDENTIFIER_WINDOW_TEST_STATE.as_bytes());
        assert_eq!(reserialized, on_disk);
    }
}
