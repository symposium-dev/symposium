//! Closed vocabulary shared by agent-originated telemetry rows.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    CohortDay, EventId, RowKind, SchemaVersion, SymposiumVersion, UtcDay, UtcSecond,
    macros::strict_versioned_row,
};
use crate::{
    agents::Agent,
    telemetry::{
        identity::{
            AgentDomain, AgentSubject, DimensionWriter, IdentifierWindowScope, IdentityDimension,
            RetentionDimension, RetentionSubject, SessionDomain, SessionId,
        },
        state::{BoundRecordingObservation, BoundSessionObservation},
    },
};

/// Agent included in the daily configuration snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum SupportedAgent {
    Claude,
    Codex,
    Copilot,
    Gemini,
    Kiro,
    #[serde(rename = "opencode")]
    OpenCode,
    Goose,
}

impl SupportedAgent {
    /// Return the frozen version 1 wire label.
    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::Kiro => "kiro",
            Self::OpenCode => "opencode",
            Self::Goose => "goose",
        }
    }
}

impl IdentityDimension for SupportedAgent {
    type Domain = AgentDomain;

    /// Write the version 1 `agent_subject` fields in contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        writer.field(self.as_str().as_bytes());
    }
}

impl From<Agent> for SupportedAgent {
    fn from(agent: Agent) -> Self {
        match agent {
            Agent::Claude => Self::Claude,
            Agent::Codex => Self::Codex,
            Agent::Copilot => Self::Copilot,
            Agent::Gemini => Self::Gemini,
            Agent::Kiro => Self::Kiro,
            Agent::OpenCode => Self::OpenCode,
            Agent::Goose => Self::Goose,
        }
    }
}

/// Agent that invoked a registered Symposium hook.
///
/// Unlike the platform enums, this has no `Other`: Symposium owns the set of
/// registered agent hooks, so adding an agent changes the row schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum HookAgent {
    Claude,
    Codex,
    Copilot,
    Gemini,
    Kiro,
}

impl HookAgent {
    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Gemini => "gemini",
            Self::Kiro => "kiro",
        }
    }
}

impl From<HookAgent> for SupportedAgent {
    fn from(agent: HookAgent) -> Self {
        match agent {
            HookAgent::Claude => Self::Claude,
            HookAgent::Codex => Self::Codex,
            HookAgent::Copilot => Self::Copilot,
            HookAgent::Gemini => Self::Gemini,
            HookAgent::Kiro => Self::Kiro,
        }
    }
}

/// A raw vendor session identifier supplied by an agent.
///
/// This value is used only as an identity-derivation input. It deliberately
/// implements neither formatting nor serialization traits so telemetry cannot
/// accidentally write it to a row or diagnostic.
pub(in crate::telemetry) struct VendorSessionId(String);

impl VendorSessionId {
    /// Wrap a vendor session identifier without changing its UTF-8 bytes.
    #[must_use]
    pub(in crate::telemetry) fn new(value: String) -> Self {
        Self(value)
    }

    /// Borrow the exact UTF-8 bytes supplied by the agent.
    #[must_use]
    fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Canonical version 1 inputs for a scoped session identifier.
struct SessionDimension<'a> {
    agent: HookAgent,
    vendor_session_id: &'a VendorSessionId,
}

impl<'a> SessionDimension<'a> {
    #[must_use]
    const fn new(agent: HookAgent, vendor_session_id: &'a VendorSessionId) -> Self {
        Self {
            agent,
            vendor_session_id,
        }
    }
}

impl IdentityDimension for SessionDimension<'_> {
    type Domain = SessionDomain;

    /// Write the version 1 `session_id` fields in contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        writer.field(self.agent.as_str().as_bytes());
        writer.field(self.vendor_session_id.as_bytes());
    }
}

/// An agent session paired with its optional scoped identifier.
///
/// The identifier is derived from the same agent stored here, so callers
/// cannot associate one agent with an identifier derived for another. Agents
/// that do not supply a vendor session identifier remain explicitly
/// unidentified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentSessionIdentity {
    agent: HookAgent,
    session_id: Option<SessionId>,
}

impl AgentSessionIdentity {
    /// Derive the identifier for one agent session in an identifier window.
    #[must_use]
    fn new(
        identity: &IdentifierWindowScope<'_>,
        agent: HookAgent,
        vendor_session_id: Option<&VendorSessionId>,
    ) -> Self {
        let session_id = vendor_session_id.map(|vendor_session_id| {
            identity.derive(&SessionDimension::new(agent, vendor_session_id))
        });

        Self { agent, session_id }
    }

    /// Return the agent whose session this identity describes.
    #[must_use]
    const fn agent(self) -> HookAgent {
        self.agent
    }

    /// Return the scoped identifier when the agent supplied a vendor id.
    #[must_use]
    const fn session_id(self) -> Option<SessionId> {
        self.session_id
    }
}

/// Derive the optional scoped session identifier used by aggregate rows.
///
/// Keeping this beside [`SessionDimension`] gives session-start and aggregate
/// rows one encoding without exposing the dimension itself.
#[must_use]
pub(super) fn derive_session_id(
    identity: &IdentifierWindowScope<'_>,
    agent: HookAgent,
    vendor_session_id: Option<&VendorSessionId>,
) -> Option<SessionId> {
    AgentSessionIdentity::new(identity, agent, vendor_session_id).session_id()
}

/// Operating-system class for the running Symposium build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum OperatingSystem {
    Linux,
    Macos,
    Windows,
    Other,
}

impl OperatingSystem {
    /// Return the contract bucket for the running Symposium build.
    #[must_use]
    pub(in crate::telemetry) fn current() -> Self {
        Self::from_target(std::env::consts::OS)
    }

    fn from_target(target: &str) -> Self {
        match target {
            "linux" => Self::Linux,
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            _ => Self::Other,
        }
    }
}

/// Architecture class for the running Symposium build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum Architecture {
    X86_64,
    Aarch64,
    Other,
}

impl Architecture {
    /// Return the contract bucket for the running Symposium build.
    #[must_use]
    pub(in crate::telemetry) fn current() -> Self {
        Self::from_target(std::env::consts::ARCH)
    }

    fn from_target(target: &str) -> Self {
        match target {
            "x86_64" => Self::X86_64,
            "aarch64" => Self::Aarch64,
            _ => Self::Other,
        }
    }
}

/// Agent-supplied classification of how a session began.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum SessionStartKind {
    Fresh,
    Resumed,
    Unknown,
}

/// Non-derived inputs for one completed session-start hook.
///
/// The raw vendor identifier is used only during subject derivation and cannot
/// be formatted or serialized as part of this input bundle.
pub(in crate::telemetry) struct SessionStartFields<'a> {
    pub(in crate::telemetry) agent: HookAgent,
    pub(in crate::telemetry) os: OperatingSystem,
    pub(in crate::telemetry) arch: Architecture,
    pub(in crate::telemetry) start: SessionStartKind,
    pub(in crate::telemetry) vendor_session_id: Option<&'a VendorSessionId>,
}

strict_versioned_row! {
    /// Version 1 record of a completed registered session-start hook.
    pub(in crate::telemetry) struct SessionStartV1 {
        at: UtcSecond,
        symposium: SymposiumVersion,
        agent: HookAgent,
        os: OperatingSystem,
        arch: Architecture,
        start: SessionStartKind,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<SessionId>,
        retention_subject: RetentionSubject,
        cohort_day: CohortDay,
    }

    kind: RowKind::SessionStart,
    raw: RawSessionStartV1,
    validate: validate_session_start,
}

impl SessionStartV1 {
    /// Create a record for a completed registered session-start hook.
    ///
    /// Both scoped identifiers and the cohort day come from the same bound
    /// state transition as its captured completion timestamp.
    #[must_use]
    pub(in crate::telemetry) fn new(
        fields: SessionStartFields<'_>,
        observation: &BoundSessionObservation<'_>,
    ) -> Self {
        let at = observation.completed_at();
        let session = AgentSessionIdentity::new(
            observation.identifier_window_scope(),
            fields.agent,
            fields.vendor_session_id,
        );
        let retention_subject = observation
            .return_cohort_scope()
            .derive(&RetentionDimension);

        Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day: at.day(),
            at,
            symposium: SymposiumVersion::current(),
            agent: session.agent(),
            os: fields.os,
            arch: fields.arch,
            start: fields.start,
            session_id: session.session_id(),
            retention_subject,
            cohort_day: observation.cohort_day(),
        }
    }
}

/// Invalid relationship between fields in a session-start row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionStartError {
    DayDoesNotMatchTimestamp { stored: UtcDay, timestamp: UtcDay },
}

impl fmt::Display for SessionStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayDoesNotMatchTimestamp { stored, timestamp } => write!(
                formatter,
                "stored session-start day {stored} does not match timestamp day {timestamp}"
            ),
        }
    }
}

impl std::error::Error for SessionStartError {}

fn validate_session_start(raw: &RawSessionStartV1) -> Result<(), SessionStartError> {
    let timestamp_day = raw.at.day();
    if raw.day != timestamp_day {
        return Err(SessionStartError::DayDoesNotMatchTimestamp {
            stored: raw.day,
            timestamp: timestamp_day,
        });
    }

    Ok(())
}

/// Fields that vary for each entry in a daily agent configuration snapshot.
///
/// These fields are repeated on [`AgentConfigurationV1`] because flattening
/// this struct into the row would weaken strict unknown-field rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) struct AgentConfigurationFields {
    pub(in crate::telemetry) agent: SupportedAgent,
    pub(in crate::telemetry) configured: bool,
}

strict_versioned_row! {
    /// Version 1 daily observation of one supported agent's configuration.
    pub(in crate::telemetry) struct AgentConfigurationV1 {
        symposium: SymposiumVersion,
        agent: SupportedAgent,
        configured: bool,
        os: OperatingSystem,
        arch: Architecture,
        agent_subject: AgentSubject,
    }

    kind: RowKind::AgentConfiguration,
    raw: RawAgentConfigurationV1,
}

impl AgentConfigurationV1 {
    /// Create one agent entry in a daily configuration snapshot.
    #[must_use]
    pub(in crate::telemetry) fn new(
        observation: &BoundRecordingObservation<'_>,
        os: OperatingSystem,
        arch: Architecture,
        fields: AgentConfigurationFields,
    ) -> Self {
        let agent_subject = observation.identifier_window_scope().derive(&fields.agent);

        Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day: observation.day(),
            symposium: SymposiumVersion::current(),
            agent: fields.agent,
            configured: fields.configured,
            os,
            arch,
            agent_subject,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::super::{
        IDENTIFIER_WINDOW_TEST_STATE, RowClassification, TelemetryRow, assert_contract_names,
        assert_contract_names_with_labels, classify_row, recording_observation,
    };
    use super::*;
    use crate::telemetry::{identity::encode_dimension_for_test, state::TelemetryStateV1};

    fn session_start_fields(
        agent: HookAgent,
        vendor_session_id: Option<&VendorSessionId>,
    ) -> SessionStartFields<'_> {
        SessionStartFields {
            agent,
            os: OperatingSystem::Linux,
            arch: Architecture::X86_64,
            start: SessionStartKind::Fresh,
            vendor_session_id,
        }
    }

    fn session_start_time() -> UtcSecond {
        UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 3, 9, 14, 2).unwrap())
    }

    fn session_start(
        completed_at: UtcSecond,
        vendor_session_id: Option<&VendorSessionId>,
    ) -> SessionStartV1 {
        session_start_for_agent(completed_at, HookAgent::Claude, vendor_session_id)
    }

    fn session_start_for_agent(
        completed_at: UtcSecond,
        agent: HookAgent,
        vendor_session_id: Option<&VendorSessionId>,
    ) -> SessionStartV1 {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = state.observe_session(completed_at).unwrap();
        let observation = state.bind_session_observation(observation).unwrap();

        SessionStartV1::new(session_start_fields(agent, vendor_session_id), &observation)
    }

    fn agent_configuration(agent: SupportedAgent) -> AgentConfigurationV1 {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);

        AgentConfigurationV1::new(
            &observation,
            OperatingSystem::Linux,
            Architecture::X86_64,
            AgentConfigurationFields {
                agent,
                configured: true,
            },
        )
    }

    #[test]
    fn hook_agents_round_trip_with_contract_names() {
        let cases = [
            (HookAgent::Claude, "claude"),
            (HookAgent::Codex, "codex"),
            (HookAgent::Copilot, "copilot"),
            (HookAgent::Gemini, "gemini"),
            (HookAgent::Kiro, "kiro"),
        ];

        assert_contract_names_with_labels(&cases, HookAgent::as_str);
    }

    #[test]
    fn session_dimension_uses_agent_then_vendor_session_id() {
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        let dimension = SessionDimension::new(HookAgent::Claude, &vendor_session_id);
        let expected = [
            [0, 0, 0, 0, 0, 0, 0, 6].as_slice(),
            b"claude".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 18].as_slice(),
            b"vendor-session-123".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&dimension);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn session_subject_derivation_matches_independent_vector() {
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());

        let row = session_start(session_start_time(), Some(&vendor_session_id));

        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, agent, and vendor session id. The complete
        // digest is
        // 2f77ea40740f4be8e85ba05e7924e1ad054037d26629db2ef7dc7097dddf723a.
        assert_eq!(
            row.session_id,
            Some("sess_2f77ea40740f4be8e85ba05e7924e1ad".parse().unwrap())
        );
    }

    #[test]
    fn session_subject_changes_with_the_agent_or_vendor_session_id() {
        let first_vendor_id = VendorSessionId::new("vendor-session-123".to_owned());
        let second_vendor_id = VendorSessionId::new("vendor-session-456".to_owned());

        let first = session_start_for_agent(
            session_start_time(),
            HookAgent::Claude,
            Some(&first_vendor_id),
        );
        let other_agent = session_start_for_agent(
            session_start_time(),
            HookAgent::Codex,
            Some(&first_vendor_id),
        );
        let other_vendor_id = session_start_for_agent(
            session_start_time(),
            HookAgent::Claude,
            Some(&second_vendor_id),
        );

        assert_ne!(first.session_id, other_agent.session_id);
        assert_ne!(first.session_id, other_vendor_id.session_id);
    }

    #[test]
    fn agent_session_without_vendor_id_is_unidentified() {
        let row = session_start_for_agent(session_start_time(), HookAgent::Copilot, None);

        assert_eq!(row.agent, HookAgent::Copilot);
        assert_eq!(row.session_id, None);
    }

    #[test]
    fn hook_agent_names_match_supported_agent_names() {
        let agents = [
            HookAgent::Claude,
            HookAgent::Codex,
            HookAgent::Copilot,
            HookAgent::Gemini,
            HookAgent::Kiro,
        ];

        for agent in agents {
            let hook_name = serde_json::to_string(&agent).unwrap();
            let supported_name = serde_json::to_string(&SupportedAgent::from(agent)).unwrap();

            assert_eq!(hook_name, supported_name);
        }
    }

    #[test]
    fn supported_agents_round_trip_with_contract_names() {
        let cases = [
            (SupportedAgent::Claude, "claude"),
            (SupportedAgent::Codex, "codex"),
            (SupportedAgent::Copilot, "copilot"),
            (SupportedAgent::Gemini, "gemini"),
            (SupportedAgent::Kiro, "kiro"),
            (SupportedAgent::OpenCode, "opencode"),
            (SupportedAgent::Goose, "goose"),
        ];

        assert_contract_names_with_labels(&cases, SupportedAgent::as_str);
    }

    #[test]
    fn project_agent_names_match_telemetry_contract_names() {
        for &agent in Agent::all() {
            let telemetry_name = serde_json::to_string(&SupportedAgent::from(agent)).unwrap();
            let config_name = format!(r#""{}""#, agent.config_name());

            assert_eq!(telemetry_name, config_name);
        }
    }

    #[test]
    fn operating_systems_round_trip_with_contract_names() {
        let cases = [
            (OperatingSystem::Linux, "linux"),
            (OperatingSystem::Macos, "macos"),
            (OperatingSystem::Windows, "windows"),
            (OperatingSystem::Other, "other"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn operating_system_target_names_map_to_contract_buckets() {
        let cases = [
            ("linux", OperatingSystem::Linux),
            ("macos", OperatingSystem::Macos),
            ("windows", OperatingSystem::Windows),
            ("freebsd", OperatingSystem::Other),
        ];

        for (target, expected) in cases {
            assert_eq!(OperatingSystem::from_target(target), expected);
        }
    }

    #[test]
    fn current_operating_system_matches_the_compile_target() {
        #[cfg(target_os = "linux")]
        let expected = OperatingSystem::Linux;
        #[cfg(target_os = "macos")]
        let expected = OperatingSystem::Macos;
        #[cfg(target_os = "windows")]
        let expected = OperatingSystem::Windows;
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        let expected = OperatingSystem::Other;

        assert_eq!(OperatingSystem::current(), expected);
    }

    #[test]
    fn architectures_round_trip_with_contract_names() {
        let cases = [
            (Architecture::X86_64, "x86_64"),
            (Architecture::Aarch64, "aarch64"),
            (Architecture::Other, "other"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn architecture_target_names_map_to_contract_buckets() {
        let cases = [
            ("x86_64", Architecture::X86_64),
            ("aarch64", Architecture::Aarch64),
            ("riscv64", Architecture::Other),
        ];

        for (target, expected) in cases {
            assert_eq!(Architecture::from_target(target), expected);
        }
    }

    #[test]
    fn current_architecture_matches_the_compile_target() {
        #[cfg(target_arch = "x86_64")]
        let expected = Architecture::X86_64;
        #[cfg(target_arch = "aarch64")]
        let expected = Architecture::Aarch64;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        let expected = Architecture::Other;

        assert_eq!(Architecture::current(), expected);
    }

    #[test]
    fn session_start_kinds_round_trip_with_contract_names() {
        let cases = [
            (SessionStartKind::Fresh, "fresh"),
            (SessionStartKind::Resumed, "resumed"),
            (SessionStartKind::Unknown, "unknown"),
        ];

        assert_contract_names(&cases);
    }

    #[test]
    fn agent_vocabulary_rejects_unknown_contract_names() {
        let unknown = r#""future_value""#;

        let supported_agent = serde_json::from_str::<SupportedAgent>(unknown);
        let hook_agent = serde_json::from_str::<HookAgent>(unknown);
        let operating_system = serde_json::from_str::<OperatingSystem>(unknown);
        let architecture = serde_json::from_str::<Architecture>(unknown);
        let start_kind = serde_json::from_str::<SessionStartKind>(unknown);

        assert!(supported_agent.is_err());
        assert!(hook_agent.is_err());
        assert!(operating_system.is_err());
        assert!(architecture.is_err());
        assert!(start_kind.is_err());
    }

    #[test]
    fn new_session_start_derives_identity_and_cohort_from_bound_observation() {
        let at = session_start_time();
        let vendor_session_id = VendorSessionId::new("vendor-session-123".to_owned());
        // These are the independently cross-checked session and retention
        // vectors pinned by the focused identity tests above and in
        // `identity.rs`.
        let expected_session = "sess_2f77ea40740f4be8e85ba05e7924e1ad".parse().unwrap();
        let expected_retention = "ret_270adecd2120c543261f04bd771df491".parse().unwrap();

        let row = session_start(at, Some(&vendor_session_id));

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::SessionStart);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, at.day());
        assert_eq!(row.at, at);
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.agent, HookAgent::Claude);
        assert_eq!(row.os, OperatingSystem::Linux);
        assert_eq!(row.arch, Architecture::X86_64);
        assert_eq!(row.start, SessionStartKind::Fresh);
        assert_eq!(row.session_id, Some(expected_session));
        assert_eq!(row.retention_subject, expected_retention);
        assert_eq!(row.cohort_day, CohortDay::D0);
    }

    #[test]
    fn session_start_before_utc_midnight_keeps_row_and_cohort_on_the_same_day() {
        let completed_at =
            UtcSecond::from_datetime(Utc.with_ymd_and_hms(2026, 8, 3, 23, 59, 59).unwrap());
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = state.observe_session(completed_at).unwrap();
        let observation = state.bind_session_observation(observation).unwrap();

        let row = SessionStartV1::new(session_start_fields(HookAgent::Claude, None), &observation);
        let stored_state = toml::to_string(&state).unwrap();
        let stored_state = toml::from_str::<toml::Value>(&stored_state).unwrap();
        let cohort_anchor = stored_state["identity"]["return-cohort-anchor"]
            .as_str()
            .unwrap();

        assert_eq!(row.at, completed_at);
        assert_eq!(row.day, completed_at.day());
        assert_eq!(cohort_anchor, row.day.to_string());
        assert_eq!(row.cohort_day, CohortDay::D0);
    }

    #[test]
    fn new_agent_configuration_derives_subject_from_its_agent() {
        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, and agent. The complete digest is
        // e346647f3c83e0f8bea71a0ff04bfb6fa601f0967a92713894dcea7f793214b0.
        let expected_subject = "agt_e346647f3c83e0f8bea71a0ff04bfb6f".parse().unwrap();

        let row = agent_configuration(SupportedAgent::Claude);

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::AgentConfiguration);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day.to_string(), "2026-08-03");
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.agent, SupportedAgent::Claude);
        assert!(row.configured);
        assert_eq!(row.os, OperatingSystem::Linux);
        assert_eq!(row.arch, Architecture::X86_64);
        assert_eq!(row.agent_subject, expected_subject);
    }

    #[test]
    fn agent_subject_changes_with_the_agent() {
        let claude = agent_configuration(SupportedAgent::Claude);
        let codex = agent_configuration(SupportedAgent::Codex);

        assert_ne!(claude.agent_subject, codex.agent_subject);
    }

    #[test]
    fn session_start_without_session_id_classifies_and_round_trips() {
        let row = session_start(session_start_time(), None);

        let json = serde_json::to_string(&row).unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        let RowClassification::Supported(TelemetryRow::SessionStart(decoded)) = classify_row(&json)
        else {
            panic!("session_start without a session id was not classified as supported");
        };

        assert_eq!(value.get("session_id"), None);
        assert!(decoded.session_id.is_none());
        assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
    }

    #[test]
    fn session_start_rejects_a_day_that_disagrees_with_its_timestamp() {
        let row = session_start(session_start_time(), None);
        let mut value = serde_json::to_value(row).unwrap();
        value["day"] = serde_json::Value::String("2026-08-04".to_owned());

        let result = serde_json::from_value::<SessionStartV1>(value);

        assert!(result.unwrap_err().to_string().contains(
            "stored session-start day 2026-08-04 does not match timestamp day 2026-08-03"
        ));
    }
}
