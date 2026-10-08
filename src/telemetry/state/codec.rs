//! Canonical, privacy-preserving encoding of private telemetry state.

use std::{error::Error, fmt, str};

use serde::Deserialize;

use super::{STATE_VERSION, TelemetryStateV1};

/// A content-free reason private state could not be decoded.
///
/// This type deliberately retains neither the state bytes nor parser errors:
/// TOML diagnostics may contain the identity key in their source line or debug
/// representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum StateContentError {
    InvalidUtf8 {
        valid_up_to: usize,
    },
    InvalidToml {
        line: Option<usize>,
        column: Option<usize>,
    },
    InvalidState {
        line: Option<usize>,
        column: Option<usize>,
    },
}

impl fmt::Display for StateContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 { valid_up_to } => write!(
                formatter,
                "telemetry private state is not valid UTF-8 near byte {valid_up_to}"
            ),
            Self::InvalidToml {
                line: Some(line),
                column: Some(column),
            } => write!(
                formatter,
                "telemetry private state contains invalid TOML at line {line}, column {column}"
            ),
            Self::InvalidToml { .. } => {
                formatter.write_str("telemetry private state contains invalid TOML")
            }
            Self::InvalidState {
                line: Some(line),
                column: Some(column),
            } => write!(
                formatter,
                "telemetry private state violates its schema at line {line}, column {column}"
            ),
            Self::InvalidState { .. } => {
                formatter.write_str("telemetry private state violates its schema")
            }
        }
    }
}

impl Error for StateContentError {}

/// Failure to decode private state after bounded filesystem reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum StateDecodeError {
    Malformed(StateContentError),
    UnsupportedVersion(u64),
}

impl fmt::Display for StateDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(error) => error.fmt(formatter),
            Self::UnsupportedVersion(version) => write!(
                formatter,
                "telemetry private state version {version} is unsupported; \
                 this build supports version {STATE_VERSION}"
            ),
        }
    }
}

impl Error for StateDecodeError {}

impl From<StateContentError> for StateDecodeError {
    fn from(error: StateContentError) -> Self {
        Self::Malformed(error)
    }
}

/// Decode and validate the complete private-state document.
///
/// Version selection happens before the selected schema is decoded so a
/// downgrade cannot mistake newer state for malformed version 1 data.
///
/// # Errors
///
/// Returns a content-free reason for invalid UTF-8, malformed TOML, invalid
/// version 1 state, or an unsupported state version.
pub(in crate::telemetry) fn decode(bytes: &[u8]) -> Result<TelemetryStateV1, StateDecodeError> {
    let source = str::from_utf8(bytes).map_err(|error| StateContentError::InvalidUtf8 {
        valid_up_to: error.valid_up_to(),
    })?;
    let envelope: StateEnvelope = parse_toml(source, ParseStage::Envelope)?;
    if envelope.version != STATE_VERSION {
        return Err(StateDecodeError::UnsupportedVersion(envelope.version));
    }

    parse_toml(source, ParseStage::State)
}

/// Encode private state in the one canonical TOML form written to disk.
///
/// # Errors
///
/// Returns the TOML serialization failure without retaining serialized state.
pub(in crate::telemetry) fn encode(state: &TelemetryStateV1) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(state)
}

#[derive(Deserialize)]
struct StateEnvelope {
    version: u64,
}

#[derive(Clone, Copy)]
enum ParseStage {
    Envelope,
    State,
}

impl ParseStage {
    fn malformed(self, line: Option<usize>, column: Option<usize>) -> StateContentError {
        match self {
            Self::Envelope => StateContentError::InvalidToml { line, column },
            Self::State => StateContentError::InvalidState { line, column },
        }
    }
}

fn parse_toml<T>(source: &str, stage: ParseStage) -> Result<T, StateDecodeError>
where
    T: for<'de> Deserialize<'de>,
{
    toml::from_str(source)
        .map_err(|error| malformed_toml(source, &error, stage))
        .map_err(Into::into)
}

fn malformed_toml(source: &str, error: &toml::de::Error, stage: ParseStage) -> StateContentError {
    let (line, column) = error
        .span()
        .and_then(|span| source_position(source, span.start))
        .map_or((None, None), |(line, column)| (Some(line), Some(column)));

    stage.malformed(line, column)
}

fn source_position(source: &str, offset: usize) -> Option<(usize, usize)> {
    let prefix = source.get(..offset)?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next()?.chars().count() + 1;
    Some((line, column))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::state::IDENTIFIER_WINDOW_TEST_STATE;

    fn assert_diagnostic_redacts(error: &StateDecodeError, sensitive: &str) {
        assert!(!error.to_string().contains(sensitive));
        assert!(!format!("{error:?}").contains(sensitive));
        assert!(error.source().is_none());
    }

    #[test]
    fn canonical_state_decodes_and_reencodes_without_drift() {
        let state = decode(IDENTIFIER_WINDOW_TEST_STATE.as_bytes()).unwrap();

        let encoded = encode(&state).unwrap();

        assert_eq!(encoded, IDENTIFIER_WINDOW_TEST_STATE);
    }

    #[test]
    fn unsupported_version_is_distinct_from_invalid_state() {
        let error = decode(b"version = 2\nfuture-field = true\n").err();

        assert_eq!(error, Some(StateDecodeError::UnsupportedVersion(2)));
    }

    #[test]
    fn diagnostics_never_retain_the_identity_key() {
        const INVALID_TOML_SECRET: &str = "recognizable-private-identity-key";
        let malformed = format!("version = 1\n\n[identity]\nkey = \"{INVALID_TOML_SECRET}\n");
        let Err(malformed_error) = decode(malformed.as_bytes()) else {
            panic!("accepted malformed private state");
        };

        assert!(matches!(
            malformed_error,
            StateDecodeError::Malformed(StateContentError::InvalidToml {
                line: Some(_),
                column: Some(_)
            })
        ));
        assert_diagnostic_redacts(&malformed_error, INVALID_TOML_SECRET);

        const INVALID_STATE_SECRET: &str = "recognizable-invalid-private-key";
        let invalid_state = format!(
            "version = 1\n\n[identity]\nkey = \"{INVALID_STATE_SECRET}\"\n\
             identifier-window-anchor = \"2026-08-03\"\n\
             latest-opened-day = \"2026-08-03\"\n"
        );
        let Err(invalid_state_error) = decode(invalid_state.as_bytes()) else {
            panic!("accepted private state containing an invalid identity key");
        };

        assert!(matches!(
            invalid_state_error,
            StateDecodeError::Malformed(StateContentError::InvalidState {
                line: Some(_),
                column: Some(_)
            })
        ));
        assert_diagnostic_redacts(&invalid_state_error, INVALID_STATE_SECRET);
    }
}
