//! Eligible command vocabulary shared by command telemetry.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{
    EventId, RowKind, SchemaVersion, SymposiumVersion, UtcDay, UtcSecond,
    extension::{PublicExtensionName, PublicExtensionNameError, PublicExtensionSource},
    macros::strict_versioned_row,
    name::{InitialByteRule, validated_string_newtype},
};
use crate::{
    cli::{Commands, PluginCommand},
    telemetry::{
        identity::{CommandDomain, CommandSubject, DimensionWriter, IdentityDimension},
        state::BoundRecordingObservation,
    },
};

const MAX_PUBLIC_COMMAND_NAME_BYTES: usize = 64;

/// Built-in command eligible for version 1 telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum BuiltinCommand {
    Init,
    Sync,
    Search,
    Use,
    Remove,
    Status,
    PluginSync,
    PluginList,
    PluginShow,
    PluginValidate,
    SelfUpdate,
    CrateInfo,
}

impl BuiltinCommand {
    /// Classify a parsed top-level CLI command for command telemetry.
    ///
    /// Hook and telemetry commands are excluded by the version 1 contract.
    /// External commands are classified separately using their public plugin
    /// provenance.
    #[must_use]
    pub(in crate::telemetry) const fn from_cli(command: &Commands) -> Option<Self> {
        match command {
            Commands::Init { .. } => Some(Self::Init),
            Commands::Sync => Some(Self::Sync),
            Commands::Search { .. } => Some(Self::Search),
            Commands::Use { remove: false, .. } => Some(Self::Use),
            Commands::Use { remove: true, .. } => Some(Self::Remove),
            Commands::Status => Some(Self::Status),
            Commands::Plugin { command } => Some(Self::from_plugin_cli(command)),
            Commands::SelfUpdate => Some(Self::SelfUpdate),
            Commands::CrateInfo { .. } => Some(Self::CrateInfo),
            Commands::Hook { .. } | Commands::Telemetry { .. } | Commands::External(_) => None,
        }
    }

    const fn from_plugin_cli(command: &PluginCommand) -> Self {
        match command {
            PluginCommand::Sync { .. } => Self::PluginSync,
            PluginCommand::List => Self::PluginList,
            PluginCommand::Show { .. } => Self::PluginShow,
            PluginCommand::Validate { .. } => Self::PluginValidate,
        }
    }

    /// Return the frozen version 1 wire label.
    #[must_use]
    pub(in crate::telemetry) const fn as_str(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Sync => "sync",
            Self::Search => "search",
            Self::Use => "use",
            Self::Remove => "remove",
            Self::Status => "status",
            Self::PluginSync => "plugin_sync",
            Self::PluginList => "plugin_list",
            Self::PluginShow => "plugin_show",
            Self::PluginValidate => "plugin_validate",
            Self::SelfUpdate => "self_update",
            Self::CrateInfo => "crate_info",
        }
    }
}

/// Closed result of an eligible command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::telemetry) enum CommandOutcome {
    Ok,
    Error,
}

validated_string_newtype! {
    /// Public plugin-command name accepted by the version 1 telemetry contract.
    pub(in crate::telemetry) struct PublicCommandName {
        error = PublicCommandNameError;
        maximum_bytes = MAX_PUBLIC_COMMAND_NAME_BYTES;
        initial_byte_rule = InitialByteRule::Alphanumeric;
        invalid_initial = NonAlphanumericFirstCharacter;
        noun = "public command name";
        as_str_doc = "Return the validated command name without changing its spelling.";
    }
}

/// Eligible public plugin command safe to place in telemetry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::telemetry) struct PublicPluginCommandCoordinate {
    source: PublicExtensionSource,
    plugin: PublicExtensionName,
    name: PublicCommandName,
}

impl PublicPluginCommandCoordinate {
    /// Combine validated components into one public plugin-command coordinate.
    #[must_use]
    pub(in crate::telemetry) const fn new(
        source: PublicExtensionSource,
        plugin: PublicExtensionName,
        name: PublicCommandName,
    ) -> Self {
        Self {
            source,
            plugin,
            name,
        }
    }

    /// Validate raw names and combine them with an allowlisted public source.
    ///
    /// # Errors
    ///
    /// Returns an error when either name is outside its version 1 public
    /// telemetry grammar.
    pub(in crate::telemetry) fn try_new(
        source: PublicExtensionSource,
        plugin: &str,
        name: &str,
    ) -> Result<Self, InvalidPublicPluginCommandCoordinate> {
        Ok(Self::new(source, plugin.parse()?, name.parse()?))
    }
}

/// Reason a public plugin-command coordinate is ineligible for telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::telemetry) enum InvalidPublicPluginCommandCoordinate {
    PluginName(PublicExtensionNameError),
    CommandName(PublicCommandNameError),
}

impl From<PublicExtensionNameError> for InvalidPublicPluginCommandCoordinate {
    fn from(error: PublicExtensionNameError) -> Self {
        Self::PluginName(error)
    }
}

impl From<PublicCommandNameError> for InvalidPublicPluginCommandCoordinate {
    fn from(error: PublicCommandNameError) -> Self {
        Self::CommandName(error)
    }
}

impl fmt::Display for InvalidPublicPluginCommandCoordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PluginName(error) => write!(formatter, "invalid plugin name: {error}"),
            Self::CommandName(error) => write!(formatter, "invalid command name: {error}"),
        }
    }
}

impl std::error::Error for InvalidPublicPluginCommandCoordinate {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PluginName(error) => Some(error),
            Self::CommandName(error) => Some(error),
        }
    }
}

/// Typed coordinate of an eligible command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::telemetry) enum CommandCoordinate {
    Builtin { name: BuiltinCommand },
    Plugin(PublicPluginCommandCoordinate),
}

impl CommandCoordinate {
    /// Build a coordinate for a fixed built-in command.
    #[must_use]
    pub(in crate::telemetry) const fn builtin(name: BuiltinCommand) -> Self {
        Self::Builtin { name }
    }

    /// Build a coordinate for an eligible public plugin command.
    #[must_use]
    pub(in crate::telemetry) const fn plugin(coordinate: PublicPluginCommandCoordinate) -> Self {
        Self::Plugin(coordinate)
    }
}

impl IdentityDimension for CommandCoordinate {
    type Domain = CommandDomain;

    /// Write the version 1 `command_subject` fields in contract order.
    fn write(&self, writer: &mut DimensionWriter<'_>) {
        match self {
            Self::Builtin { name } => writer.variant("builtin", |writer| {
                writer.field(name.as_str().as_bytes());
            }),
            Self::Plugin(coordinate) => writer.variant("plugin", |writer| {
                writer.field(coordinate.source.as_str().as_bytes());
                writer.field(coordinate.plugin.as_str().as_bytes());
                writer.field(coordinate.name.as_str().as_bytes());
            }),
        }
    }
}

strict_versioned_row! {
    /// Version 1 record of one completed eligible top-level command.
    pub(in crate::telemetry) struct CommandV1 {
        at: UtcSecond,
        symposium: SymposiumVersion,
        command: CommandCoordinate,
        duration_ms: u64,
        outcome: CommandOutcome,
        command_subject: CommandSubject,
    }

    kind: RowKind::Command,
    raw: RawCommandV1,
    error: CommandError,
    validate: validate_command,
}

impl CommandV1 {
    /// Create a record for one completed eligible top-level command.
    #[must_use]
    pub(in crate::telemetry) fn new(
        observation: &BoundRecordingObservation<'_>,
        command: CommandCoordinate,
        duration_ms: u64,
        outcome: CommandOutcome,
    ) -> Self {
        let at = observation.completed_at();
        let day = observation.day();
        let command_subject = observation.identifier_window_scope().derive(&command);

        Self {
            version: SchemaVersion::V1,
            kind: Self::KIND,
            event_id: EventId::new(),
            day,
            at,
            symposium: SymposiumVersion::current(),
            command,
            duration_ms,
            outcome,
            command_subject,
        }
    }
}

fn validate_command(raw: &RawCommandV1) -> Result<(), CommandError> {
    let timestamp_day = raw.at.day();
    if raw.day != timestamp_day {
        return Err(CommandError::DayDoesNotMatchTimestamp {
            stored: raw.day,
            timestamp: timestamp_day,
        });
    }

    Ok(())
}

/// Invalid relationship between fields in a command row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandError {
    DayDoesNotMatchTimestamp { stored: UtcDay, timestamp: UtcDay },
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DayDoesNotMatchTimestamp { stored, timestamp } => write!(
                formatter,
                "stored command day {stored} does not match timestamp day {timestamp}"
            ),
        }
    }
}

impl std::error::Error for CommandError {}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::super::{
        IDENTIFIER_WINDOW_TEST_STATE, RowClassification, TelemetryRow, assert_contract_names,
        assert_contract_names_with_labels, classify_row, recorded_data_example_block_at,
        recorded_data_example_row, recording_observation,
    };
    use super::*;
    use crate::{
        cli::Cli,
        telemetry::{identity::encode_dimension_for_test, state::TelemetryStateV1},
    };

    fn parse_command(arguments: &[&str]) -> Commands {
        Cli::try_parse_from(std::iter::once("cargo-agents").chain(arguments.iter().copied()))
            .unwrap()
            .command
            .unwrap()
    }

    fn plugin_command(
        source: PublicExtensionSource,
        plugin: &str,
        name: &str,
    ) -> CommandCoordinate {
        CommandCoordinate::plugin(
            PublicPluginCommandCoordinate::try_new(source, plugin, name).unwrap(),
        )
    }

    fn command_row(command: CommandCoordinate) -> CommandV1 {
        let mut state: TelemetryStateV1 = toml::from_str(IDENTIFIER_WINDOW_TEST_STATE).unwrap();
        let observation = recording_observation(&mut state);

        CommandV1::new(&observation, command, 820, CommandOutcome::Ok)
    }

    #[test]
    fn cli_commands_map_exhaustively_to_telemetry_builtins() {
        let cases: &[(&[&str], Option<BuiltinCommand>)] = &[
            (&["init"], Some(BuiltinCommand::Init)),
            (&["sync"], Some(BuiltinCommand::Sync)),
            (&["search", "serde"], Some(BuiltinCommand::Search)),
            (&["use", "serde"], Some(BuiltinCommand::Use)),
            (&["use", "serde", "--remove"], Some(BuiltinCommand::Remove)),
            (&["status"], Some(BuiltinCommand::Status)),
            (&["plugin", "sync"], Some(BuiltinCommand::PluginSync)),
            (&["plugin", "list"], Some(BuiltinCommand::PluginList)),
            (
                &["plugin", "show", "example-tools"],
                Some(BuiltinCommand::PluginShow),
            ),
            (
                &["plugin", "validate", "."],
                Some(BuiltinCommand::PluginValidate),
            ),
            (&["self-update"], Some(BuiltinCommand::SelfUpdate)),
            (&["crate-info", "serde"], Some(BuiltinCommand::CrateInfo)),
            (&["hook", "claude", "session-start"], None),
            (&["telemetry"], None),
            (&["example-check"], None),
        ];

        for &(arguments, expected) in cases {
            let command = parse_command(arguments);

            assert_eq!(BuiltinCommand::from_cli(&command), expected);
        }
    }

    #[test]
    fn builtin_commands_round_trip_with_contract_names() {
        let cases = [
            (BuiltinCommand::Init, "init"),
            (BuiltinCommand::Sync, "sync"),
            (BuiltinCommand::Search, "search"),
            (BuiltinCommand::Use, "use"),
            (BuiltinCommand::Remove, "remove"),
            (BuiltinCommand::Status, "status"),
            (BuiltinCommand::PluginSync, "plugin_sync"),
            (BuiltinCommand::PluginList, "plugin_list"),
            (BuiltinCommand::PluginShow, "plugin_show"),
            (BuiltinCommand::PluginValidate, "plugin_validate"),
            (BuiltinCommand::SelfUpdate, "self_update"),
            (BuiltinCommand::CrateInfo, "crate_info"),
        ];

        assert_contract_names_with_labels(&cases, BuiltinCommand::as_str);
    }

    #[test]
    fn command_outcomes_round_trip_with_contract_names() {
        let cases = [(CommandOutcome::Ok, "ok"), (CommandOutcome::Error, "error")];

        assert_contract_names(&cases);
    }

    #[test]
    fn command_vocabulary_rejects_unknown_contract_names() {
        let builtin = serde_json::from_str::<BuiltinCommand>(r#""telemetry""#);
        let outcome = serde_json::from_str::<CommandOutcome>(r#""cancelled""#);

        assert!(builtin.is_err());
        assert!(outcome.is_err());
    }

    #[test]
    fn public_command_names_accept_the_contract_grammar() {
        for value in ["0", "Example-check_2", &"a".repeat(64)] {
            let name = value.parse::<PublicCommandName>().unwrap();

            assert_eq!(name.as_str(), value);
        }
    }

    #[test]
    fn public_command_names_reject_invalid_length() {
        let empty = "".parse::<PublicCommandName>();
        let too_long = "a".repeat(65).parse::<PublicCommandName>();

        assert_eq!(empty.unwrap_err(), PublicCommandNameError::Empty);
        assert_eq!(too_long.unwrap_err(), PublicCommandNameError::TooLong);
    }

    #[test]
    fn public_command_names_require_an_ascii_alphanumeric_first_byte() {
        for value in ["-check", "_check", "\u{e9}check"] {
            let result = value.parse::<PublicCommandName>();

            assert_eq!(
                result.unwrap_err(),
                PublicCommandNameError::NonAlphanumericFirstCharacter
            );
        }
    }

    #[test]
    fn public_command_names_reject_unsupported_characters() {
        for value in ["plugin.check", "plugin check", "plugin/check", "a\u{e9}"] {
            let result = value.parse::<PublicCommandName>();

            assert_eq!(
                result.unwrap_err(),
                PublicCommandNameError::UnsupportedCharacter
            );
        }
    }

    #[test]
    fn public_command_names_round_trip_without_normalization() {
        let name = "Example-check_2".parse::<PublicCommandName>().unwrap();

        let encoded = serde_json::to_string(&name).unwrap();
        let decoded = serde_json::from_str::<PublicCommandName>(&encoded).unwrap();

        assert_eq!(encoded, r#""Example-check_2""#);
        assert_eq!(decoded, name);
    }

    #[test]
    fn plugin_command_coordinate_round_trips_in_contract_shape() {
        let command = plugin_command(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
            "example-check",
        );

        let json = serde_json::to_string(&command).unwrap();
        let decoded = serde_json::from_str::<CommandCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            recorded_data_example_block_at("### `command`", "```json", 1).trim()
        );
        assert_eq!(decoded, command);
    }

    #[test]
    fn builtin_command_coordinate_round_trips_in_contract_shape() {
        let command = CommandCoordinate::builtin(BuiltinCommand::Use);

        let json = serde_json::to_string(&command).unwrap();
        let decoded = serde_json::from_str::<CommandCoordinate>(&json).unwrap();

        assert_eq!(
            json,
            recorded_data_example_block_at("### `command`", "```json", 0).trim()
        );
        assert_eq!(decoded, command);
    }

    #[test]
    fn builtin_command_subject_dimension_uses_type_then_name() {
        let command = CommandCoordinate::builtin(BuiltinCommand::Use);
        let expected = [
            [0, 0, 0, 0, 0, 0, 0, 7].as_slice(),
            b"builtin".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 3].as_slice(),
            b"use".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&command);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn plugin_command_subject_dimension_uses_contract_field_order() {
        let command = plugin_command(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
            "example-check",
        );
        let expected = [
            [0, 0, 0, 0, 0, 0, 0, 6].as_slice(),
            b"plugin".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 25].as_slice(),
            b"symposium-recommendations".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 13].as_slice(),
            b"example-tools".as_slice(),
            [0, 0, 0, 0, 0, 0, 0, 13].as_slice(),
            b"example-check".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&command);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn command_subject_derivation_matches_independent_vector() {
        let command = plugin_command(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
            "example-check",
        );

        let subject = command_row(command).command_subject;

        // Cross-checked with .NET's HMACSHA256 over the contract header,
        // identifier window, command type, source, plugin name, and command
        // name. The complete digest is
        // c50f828e42f9eb719589039d90da38fa69c82f644689a19ce34562513a236c41.
        assert_eq!(
            subject,
            "cmd_c50f828e42f9eb719589039d90da38fa".parse().unwrap()
        );
    }

    #[test]
    fn command_subject_changes_with_the_typed_coordinate() {
        let baseline = plugin_command(
            PublicExtensionSource::SymposiumRecommendations,
            "example-tools",
            "example-check",
        );
        let changed_coordinates = [
            CommandCoordinate::builtin(BuiltinCommand::Use),
            plugin_command(
                PublicExtensionSource::CratesIo,
                "example-tools",
                "example-check",
            ),
            plugin_command(
                PublicExtensionSource::SymposiumRecommendations,
                "other-tools",
                "example-check",
            ),
            plugin_command(
                PublicExtensionSource::SymposiumRecommendations,
                "example-tools",
                "other-check",
            ),
        ];

        let baseline_subject = command_row(baseline).command_subject;
        let use_subject =
            command_row(CommandCoordinate::builtin(BuiltinCommand::Use)).command_subject;
        let remove_subject =
            command_row(CommandCoordinate::builtin(BuiltinCommand::Remove)).command_subject;

        for coordinate in changed_coordinates {
            assert_ne!(command_row(coordinate).command_subject, baseline_subject);
        }
        assert_ne!(use_subject, remove_subject);
    }

    #[test]
    fn command_example_round_trips_in_contract_shape() {
        let source = recorded_data_example_row("command");

        let RowClassification::Supported(TelemetryRow::Command(row)) = classify_row(source) else {
            panic!("documented command row was not classified as supported");
        };
        let serialized = serde_json::to_string(&row).unwrap();

        assert_eq!(serialized, source);
    }

    #[test]
    fn new_command_derives_fixed_fields_day_and_subject() {
        let command = CommandCoordinate::builtin(BuiltinCommand::Use);
        // Cross-checked in the same independent .NET calculation as the
        // plugin-command vector. The complete digest is
        // 0b0899a55d1cbe757b8b505091f6e1f3a4646f26a9a05de1769faaea84f249ca.
        let expected_subject = "cmd_0b0899a55d1cbe757b8b505091f6e1f3".parse().unwrap();

        let row = command_row(command.clone());

        assert_eq!(row.version, SchemaVersion::V1);
        assert_eq!(row.kind, RowKind::Command);
        assert_eq!(row.event_id.0.get_version(), Some(uuid::Version::Random));
        assert_eq!(row.day, row.at.day());
        assert_eq!(
            serde_json::to_string(&row.at).unwrap(),
            r#""2026-08-03T10:02:11Z""#
        );
        assert_eq!(row.symposium, SymposiumVersion::current());
        assert_eq!(row.command, command);
        assert_eq!(row.duration_ms, 820);
        assert_eq!(row.outcome, CommandOutcome::Ok);
        assert_eq!(row.command_subject, expected_subject);
    }

    #[test]
    fn command_rejects_a_day_that_disagrees_with_its_timestamp() {
        let row = command_row(CommandCoordinate::builtin(BuiltinCommand::Use));
        let mut value = serde_json::to_value(row).unwrap();
        value["day"] = serde_json::Value::String("2026-08-04".to_owned());

        let result = serde_json::from_value::<CommandV1>(value);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("stored command day 2026-08-04 does not match timestamp day 2026-08-03")
        );
    }

    #[test]
    fn command_rejects_future_version() {
        let row = command_row(CommandCoordinate::builtin(BuiltinCommand::Use));
        let mut value = serde_json::to_value(row).unwrap();
        value["v"] = serde_json::Value::from(2);

        let result = serde_json::from_value::<CommandV1>(value);

        assert!(result.is_err());
    }

    #[test]
    fn command_rejects_another_row_kind() {
        let row = command_row(CommandCoordinate::builtin(BuiltinCommand::Use));
        let mut value = serde_json::to_value(row).unwrap();
        value["kind"] = serde_json::Value::String("session_start".to_owned());

        let result = serde_json::from_value::<CommandV1>(value);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected Command row kind, found SessionStart")
        );
    }

    #[test]
    fn command_rejects_unknown_fields() {
        let row = command_row(CommandCoordinate::builtin(BuiltinCommand::Use));
        let mut value = serde_json::to_value(row).unwrap();
        value["arguments"] = serde_json::Value::String("example-tools".to_owned());

        let result = serde_json::from_value::<CommandV1>(value);

        assert!(result.is_err());
    }

    #[test]
    fn command_requires_every_field() {
        let row = command_row(CommandCoordinate::builtin(BuiltinCommand::Use));
        let mut value = serde_json::to_value(row).unwrap();
        value.as_object_mut().unwrap().remove("duration_ms");

        let result = serde_json::from_value::<CommandV1>(value);

        assert!(result.is_err());
    }

    #[test]
    fn plugin_command_coordinate_validates_both_names() {
        let invalid_plugin = PublicPluginCommandCoordinate::try_new(
            PublicExtensionSource::CratesIo,
            "private/plugin",
            "check",
        );
        let invalid_command = PublicPluginCommandCoordinate::try_new(
            PublicExtensionSource::CratesIo,
            "example-tools",
            "private/check",
        );
        let invalid_json = serde_json::from_str::<CommandCoordinate>(
            r#"{"type":"plugin","source":"crates-io","plugin":"example-tools","name":"private/check"}"#,
        );

        assert_eq!(
            invalid_plugin.unwrap_err(),
            InvalidPublicPluginCommandCoordinate::PluginName(
                PublicExtensionNameError::UnsupportedCharacter
            )
        );
        assert_eq!(
            invalid_command.unwrap_err(),
            InvalidPublicPluginCommandCoordinate::CommandName(
                PublicCommandNameError::UnsupportedCharacter
            )
        );
        assert!(invalid_json.is_err());
    }

    #[test]
    fn command_coordinates_reject_unknown_or_missing_fields() {
        let unknown_builtin_field = serde_json::from_str::<CommandCoordinate>(
            r#"{"type":"builtin","name":"use","args":"example-tools"}"#,
        );
        let unknown_plugin_field = serde_json::from_str::<CommandCoordinate>(
            r#"{"type":"plugin","source":"crates-io","plugin":"example-tools","name":"check","args":"--all"}"#,
        );
        let missing_field = serde_json::from_str::<CommandCoordinate>(
            r#"{"type":"plugin","source":"crates-io","plugin":"example-tools"}"#,
        );
        let unknown_type =
            serde_json::from_str::<CommandCoordinate>(r#"{"type":"external","name":"check"}"#);

        assert!(unknown_builtin_field.is_err());
        assert!(unknown_plugin_field.is_err());
        assert!(missing_field.is_err());
        assert!(unknown_type.is_err());
    }
}
