//! Domain-specific identifiers used by telemetry rows.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "identifier types are built before telemetry producers use them."
    )
)]

use std::{
    cmp::Ordering,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    str::FromStr,
};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::Sha256;

const IDENTIFIER_BYTES: usize = 16;
const ENCODED_DIGITS: usize = IDENTIFIER_BYTES * 2;
const IDENTITY_KEY_BYTES: usize = 32;

type HmacSha256 = Hmac<Sha256>;

/// A 256-bit secret used to derive telemetry pseudonyms.
///
/// This type deliberately implements no formatting traits, which prevents the
/// key from being printed accidentally in diagnostics. It does not promise to
/// scrub every in-memory copy when dropped.
pub(super) struct IdentityKey([u8; IDENTITY_KEY_BYTES]);

impl IdentityKey {
    #[must_use]
    const fn from_bytes(bytes: [u8; IDENTITY_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Generate a key from the operating system's preferred random source.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system cannot provide random bytes.
    pub(super) fn generate() -> Result<Self, getrandom::Error> {
        Self::generate_with(getrandom::fill)
    }

    pub(super) fn generate_with<E>(
        fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
    ) -> Result<Self, E> {
        let mut bytes = [0; IDENTITY_KEY_BYTES];
        fill(&mut bytes)?;
        Ok(Self::from_bytes(bytes))
    }
}

/// Canonical bytes for an identifier-window or return-cohort anchor.
///
/// Keeping this distinct from a dimension makes their order in the HMAC input
/// impossible to swap accidentally.
struct IdentityWindow<'a>(&'a [u8]);

#[cfg(test)]
impl<'a> IdentityWindow<'a> {
    #[must_use]
    const fn from_bytes(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }
}

/// Canonically framed dimension fields belonging to domain `D`.
///
/// Domain-specific constructors will own field selection and order. Telemetry
/// producers never concatenate dimension strings themselves.
struct ScopedDimension<D> {
    encoded_fields: Vec<u8>,
    domain: PhantomData<D>,
}

#[cfg(test)]
impl<D> ScopedDimension<D> {
    #[must_use]
    fn from_fields<'a>(fields: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut encoded_fields = Vec::new();
        for field in fields {
            write_frame(field, |bytes| encoded_fields.extend_from_slice(bytes));
        }

        Self {
            encoded_fields,
            domain: PhantomData,
        }
    }
}

/// A 128-bit telemetry identifier belonging to domain `D`.
///
/// Its wire form is the domain prefix followed by `ENCODED_DIGITS` lowercase
/// hexadecimal digits.
pub(super) struct ScopedId<D> {
    bytes: [u8; IDENTIFIER_BYTES],
    domain: PhantomData<D>,
}

impl<D> ScopedId<D> {
    /// Wrap the leading 128 bits of a derived pseudonym.
    ///
    /// Private, so identifier derivation has to live in this module rather than
    /// anywhere in telemetry that happens to hold sixteen bytes.
    #[must_use]
    const fn from_bytes(bytes: [u8; IDENTIFIER_BYTES]) -> Self {
        Self {
            bytes,
            domain: PhantomData,
        }
    }
}

// Written out rather than derived: a derive puts the same bound on `D`, so an
// identifier would only gain each trait when its zero-sized marker declared it.
impl<D> Clone for ScopedId<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D> Copy for ScopedId<D> {}

impl<D> PartialEq for ScopedId<D> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<D> Eq for ScopedId<D> {}

impl<D> PartialOrd for ScopedId<D> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<D> Ord for ScopedId<D> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl<D> Hash for ScopedId<D> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

/// Marker supplying a [`ScopedId`] domain's frozen derivation and wire labels.
///
/// Private, which seals it: both constants are published contract surfaces,
/// not extension points. Changing either requires a new consent version.
trait ScopedIdDomain {
    const PREFIX: &'static str;
    const HMAC_DOMAIN: &'static str;
}

/// Derives purpose-scoped telemetry identifiers from one secret key.
struct IdentityDeriver {
    key: IdentityKey,
}

impl IdentityDeriver {
    #[must_use]
    const fn new(key: IdentityKey) -> Self {
        Self { key }
    }

    /// Derive an identifier from a canonical window and domain-specific fields.
    #[must_use]
    fn derive<D>(&self, window: &IdentityWindow<'_>, dimension: &ScopedDimension<D>) -> ScopedId<D>
    where
        D: ScopedIdDomain,
    {
        let mut hmac = HmacSha256::new_from_slice(&self.key.0)
            .expect("BUG: HMAC-SHA-256 must accept a 32-byte key");
        hmac.update(b"telemetry:");
        hmac.update(D::HMAC_DOMAIN.as_bytes());
        hmac.update(b":v1\0");
        write_frame(window.0, |bytes| hmac.update(bytes));
        hmac.update(&dimension.encoded_fields);

        let digest = hmac.finalize().into_bytes();
        let mut bytes = [0; IDENTIFIER_BYTES];
        bytes.copy_from_slice(&digest[..IDENTIFIER_BYTES]);
        ScopedId::from_bytes(bytes)
    }
}

/// Write one unambiguous variable-length value to an HMAC input or buffer.
fn write_frame(value: &[u8], mut write: impl FnMut(&[u8])) {
    let length = u64::try_from(value.len())
        .expect("BUG: a slice length must fit the telemetry frame format");
    write(&length.to_be_bytes());
    write(value);
}

/// Reason a stored scoped identifier is not canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParseScopedIdError {
    IncorrectPrefix,
    IncorrectLength,
    InvalidHex,
}

impl fmt::Display for ParseScopedIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncorrectPrefix => formatter.write_str("identifier has the wrong domain prefix"),
            Self::IncorrectLength => write!(
                formatter,
                "identifier must contain exactly {ENCODED_DIGITS} hexadecimal digits"
            ),
            Self::InvalidHex => {
                formatter.write_str("identifier contains a non-lowercase-hexadecimal character")
            }
        }
    }
}

impl std::error::Error for ParseScopedIdError {}

// Debug prints the wire form too: the derived one dumps sixteen decimal numbers,
// which makes a failed identifier comparison unreadable.
impl<D> fmt::Debug for ScopedId<D>
where
    D: ScopedIdDomain,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self}")
    }
}

impl<D> fmt::Display for ScopedId<D>
where
    D: ScopedIdDomain,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(D::PREFIX)?;
        write_lower_hex(&self.bytes, formatter)
    }
}

impl<D> FromStr for ScopedId<D>
where
    D: ScopedIdDomain,
{
    type Err = ParseScopedIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let encoded = value
            .strip_prefix(D::PREFIX)
            .ok_or(ParseScopedIdError::IncorrectPrefix)?;

        let bytes = decode_lower_hex_array(encoded).map_err(|error| match error {
            DecodeLowerHexError::IncorrectLength => ParseScopedIdError::IncorrectLength,
            DecodeLowerHexError::InvalidDigit => ParseScopedIdError::InvalidHex,
        })?;

        Ok(Self::from_bytes(bytes))
    }
}

impl<D> Serialize for ScopedId<D>
where
    D: ScopedIdDomain,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de, D> Deserialize<'de> for ScopedId<D>
where
    D: ScopedIdDomain,
{
    fn deserialize<De>(deserializer: De) -> Result<Self, De::Error>
    where
        De: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(De::Error::custom)
    }
}

/// Write `bytes` as lowercase hexadecimal digits.
///
/// Takes any [`fmt::Write`] so a formatter and a string buffer share one encoder.
fn write_lower_hex(bytes: &[u8], out: &mut impl fmt::Write) -> fmt::Result {
    for byte in bytes {
        write!(out, "{byte:02x}")?;
    }

    Ok(())
}

/// Reason lowercase hexadecimal digits do not decode to a fixed byte array.
///
/// Deliberately unnamed in user-facing text: each caller names the failure in
/// terms of the value it was parsing.
enum DecodeLowerHexError {
    IncorrectLength,
    InvalidDigit,
}

/// Decode `N` bytes from exactly `2 * N` lowercase hexadecimal digits.
fn decode_lower_hex_array<const N: usize>(encoded: &str) -> Result<[u8; N], DecodeLowerHexError> {
    if encoded.len() != N * 2 {
        return Err(DecodeLowerHexError::IncorrectLength);
    }

    // The length check leaves no remainder, so every digit reaches a pair.
    let (digit_pairs, _) = encoded.as_bytes().as_chunks::<2>();

    let mut bytes = [0; N];
    for (&[high, low], output) in digit_pairs.iter().zip(&mut bytes) {
        let high = decode_lower_hex_digit(high).ok_or(DecodeLowerHexError::InvalidDigit)?;
        let low = decode_lower_hex_digit(low).ok_or(DecodeLowerHexError::InvalidDigit)?;
        *output = (high << 4) | low;
    }

    Ok(bytes)
}

fn decode_lower_hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

/// Serde adapter for the identity key stored in `telemetry-state.toml`.
///
/// Keeping this adapter here avoids giving [`IdentityKey`] general-purpose
/// formatting or serialization traits that could expose it elsewhere. Writing
/// the key does materialize it as a hexadecimal string, which is not scrubbed:
/// see the boundary documented on [`IdentityKey`].
pub(super) mod state_key_hex {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    use super::{
        DecodeLowerHexError, IDENTITY_KEY_BYTES, IdentityKey, decode_lower_hex_array,
        write_lower_hex,
    };

    const ENCODED_KEY_DIGITS: usize = IDENTITY_KEY_BYTES * 2;

    pub(in crate::telemetry) fn serialize<S>(
        key: &IdentityKey,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = String::with_capacity(ENCODED_KEY_DIGITS);
        write_lower_hex(&key.0, &mut encoded).expect("BUG: writing to a String cannot fail");

        serializer.serialize_str(&encoded)
    }

    pub(in crate::telemetry) fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<IdentityKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;

        let bytes =
            decode_lower_hex_array::<IDENTITY_KEY_BYTES>(&encoded).map_err(
                |error| match error {
                    DecodeLowerHexError::IncorrectLength => D::Error::custom(format_args!(
                        "identity key must contain exactly {ENCODED_KEY_DIGITS} hexadecimal digits"
                    )),
                    DecodeLowerHexError::InvalidDigit => {
                        D::Error::custom("identity key must use lowercase hexadecimal digits")
                    }
                },
            )?;

        Ok(IdentityKey::from_bytes(bytes))
    }
}

/// Declares each identifier domain, its contract prefix, and its alias.
///
/// A macro because a function cannot introduce types, and the prefix is the
/// hand-written part worth handing to the tests as a table.
macro_rules! scoped_id_domains {
    (
        $(
            $domain:ident => $alias:ident {
                hmac_domain: $hmac_domain:literal,
                wire_prefix: $prefix:literal,
            }
        )+
    ) => {
        $(
            pub(super) enum $domain {}

            impl ScopedIdDomain for $domain {
                const PREFIX: &'static str = $prefix;
                const HMAC_DOMAIN: &'static str = $hmac_domain;
            }

            pub(super) type $alias = ScopedId<$domain>;
        )+

        #[cfg(test)]
        const PREFIX_PARSERS: &[(&str, fn(&str) -> bool)] = &[
            $(($prefix, |value| value.parse::<$alias>().is_ok())),+
        ];

        #[cfg(test)]
        const DOMAIN_CONTRACTS: &[(&str, &str)] = &[
            $(($hmac_domain, $prefix)),+
        ];
    };
}

scoped_id_domains! {
    SessionDomain => SessionId {
        hmac_domain: "session_id",
        wire_prefix: "sess_",
    }
    RetentionDomain => RetentionSubject {
        hmac_domain: "retention_subject",
        wire_prefix: "ret_",
    }
    AgentDomain => AgentSubject {
        hmac_domain: "agent_subject",
        wire_prefix: "agt_",
    }
    PackageDomain => PackageSubject {
        hmac_domain: "package_subject",
        wire_prefix: "pkg_",
    }
    ExtensionDomain => ExtensionSubject {
        hmac_domain: "extension_subject",
        wire_prefix: "ext_",
    }
    HookDomain => HookSubject {
        hmac_domain: "hook_subject",
        wire_prefix: "hok_",
    }
    PluginDomain => PluginSubject {
        hmac_domain: "plugin_subject",
        wire_prefix: "plg_",
    }
    CommandDomain => CommandSubject {
        hmac_domain: "command_subject",
        wire_prefix: "cmd_",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::TypeId,
        collections::HashSet,
        mem::{size_of, size_of_val},
    };

    use super::*;

    const RECORDED_DATA: &str =
        include_str!("../../md/rfds/telemetry-recording/contract/recorded-data.md");

    const TEST_HEX: &str = "00112233445566778899aabbccddeeff";

    const TEST_BYTES: [u8; IDENTIFIER_BYTES] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff,
    ];

    enum TestDomain {}

    impl ScopedIdDomain for TestDomain {
        const PREFIX: &'static str = "test_";
        const HMAC_DOMAIN: &'static str = "test_subject";
    }

    /// Reads the first identifier the contract spells with `prefix`.
    fn contract_identifier(prefix: &str) -> &'static str {
        let opening_quote = RECORDED_DATA
            .find(&format!("\"{prefix}"))
            .unwrap_or_else(|| panic!("recorded-data contract names no {prefix} identifier"));

        let value = &RECORDED_DATA[opening_quote + 1..];
        let closing_quote = value
            .find('"')
            .expect("recorded-data identifier must be terminated");

        &value[..closing_quote]
    }

    #[test]
    fn identity_key_wraps_exactly_32_bytes() {
        let bytes = [0x5a; IDENTITY_KEY_BYTES];

        let key = IdentityKey::from_bytes(bytes);

        assert_eq!(key.0, bytes);
        assert_eq!(size_of::<IdentityKey>(), IDENTITY_KEY_BYTES);
    }

    #[test]
    fn identity_key_generation_fills_the_complete_key() {
        let expected = [0x5a; IDENTITY_KEY_BYTES];

        let key = IdentityKey::generate_with(|bytes| {
            bytes.copy_from_slice(&expected);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("infallible test source must generate a key");

        assert_eq!(key.0, expected);
    }

    #[test]
    fn identity_key_generation_propagates_source_failure() {
        #[derive(Debug, PartialEq, Eq)]
        struct TestError;

        let result = IdentityKey::generate_with(|_| Err(TestError));

        assert!(matches!(result, Err(TestError)));
    }

    #[test]
    fn identity_key_can_be_generated_from_the_operating_system() {
        let key = IdentityKey::generate().expect("operating system must provide random bytes");

        assert_eq!(size_of_val(&key), IDENTITY_KEY_BYTES);
    }

    #[test]
    fn identity_derivation_matches_contract_vector() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let deriver = IdentityDeriver::new(key);
        let window = IdentityWindow::from_bytes(b"window-1");
        let dimension = ScopedDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);

        let identifier = deriver.derive(&window, &dimension);

        // Cross-checked with .NET's HMACSHA256 over the contract's header and
        // two unsigned 64-bit big-endian length-prefixed values. The complete
        // digest is ec1c11acdcca37eb4ab17f381c80df5246b0a66438f757eaa2a52195da40afbd.
        assert_eq!(
            identifier.to_string(),
            "sess_ec1c11acdcca37eb4ab17f381c80df52"
        );
    }

    #[test]
    fn identical_derivation_inputs_are_stable() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let deriver = IdentityDeriver::new(key);
        let window = IdentityWindow::from_bytes(b"window-1");
        let dimension = ScopedDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);

        let first = deriver.derive(&window, &dimension);
        let second = deriver.derive(&window, &dimension);

        assert_eq!(first, second);
    }

    #[test]
    fn length_framing_separates_nul_at_different_boundaries() {
        let deriver = IdentityDeriver::new(IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]));
        let first_window = IdentityWindow::from_bytes(b"a\0b");
        let first_dimension = ScopedDimension::<SessionDomain>::from_fields([b"c".as_slice()]);
        let second_window = IdentityWindow::from_bytes(b"a");
        let second_dimension = ScopedDimension::<SessionDomain>::from_fields([b"b\0c".as_slice()]);

        let first = deriver.derive(&first_window, &first_dimension);
        let second = deriver.derive(&second_window, &second_dimension);

        assert_ne!(first, second);
    }

    #[test]
    fn length_framing_separates_dimension_field_boundaries() {
        let deriver = IdentityDeriver::new(IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]));
        let window = IdentityWindow::from_bytes(b"window-1");
        let first_dimension = ScopedDimension::<PackageDomain>::from_fields([
            b"cargo".as_slice(),
            b"foo1".as_slice(),
            b"2.3.4".as_slice(),
        ]);
        let second_dimension = ScopedDimension::<PackageDomain>::from_fields([
            b"cargo".as_slice(),
            b"foo".as_slice(),
            b"12.3.4".as_slice(),
        ]);

        let first = deriver.derive(&window, &first_dimension);
        let second = deriver.derive(&window, &second_dimension);

        assert_ne!(first, second);
    }

    #[test]
    fn changing_any_derivation_scope_changes_the_identifier() {
        let deriver = IdentityDeriver::new(IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]));
        let other_deriver =
            IdentityDeriver::new(IdentityKey::from_bytes([0x24; IDENTITY_KEY_BYTES]));
        let first_window = IdentityWindow::from_bytes(b"window-1");
        let second_window = IdentityWindow::from_bytes(b"window-2");
        let session_dimension =
            ScopedDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);
        let other_session_dimension =
            ScopedDimension::<SessionDomain>::from_fields([b"dimension-2".as_slice()]);
        let command_dimension =
            ScopedDimension::<CommandDomain>::from_fields([b"dimension-1".as_slice()]);

        // Different domain markers produce different identifier types, so use
        // their shared byte representation for this one collection.
        let identifiers = [
            deriver.derive(&first_window, &session_dimension).bytes,
            other_deriver
                .derive(&first_window, &session_dimension)
                .bytes,
            deriver.derive(&first_window, &command_dimension).bytes,
            deriver.derive(&second_window, &session_dimension).bytes,
            deriver
                .derive(&first_window, &other_session_dimension)
                .bytes,
        ];

        let unique_identifiers = identifiers.into_iter().collect::<HashSet<_>>();

        assert_eq!(unique_identifiers.len(), identifiers.len());
    }

    #[test]
    fn domain_marker_adds_no_storage_to_identifier() {
        let bytes = [0x5a; IDENTIFIER_BYTES];

        let identifier = ScopedId::<TestDomain>::from_bytes(bytes);

        assert_eq!(identifier, ScopedId::<TestDomain>::from_bytes(bytes));
        assert_eq!(size_of::<ScopedId<TestDomain>>(), IDENTIFIER_BYTES);
    }

    #[test]
    fn identifier_domains_are_distinct_types() {
        let domains = [
            TypeId::of::<SessionId>(),
            TypeId::of::<RetentionSubject>(),
            TypeId::of::<AgentSubject>(),
            TypeId::of::<PackageSubject>(),
            TypeId::of::<ExtensionSubject>(),
            TypeId::of::<HookSubject>(),
            TypeId::of::<PluginSubject>(),
            TypeId::of::<CommandSubject>(),
        ];

        let unique_domains = domains.into_iter().collect::<HashSet<_>>();

        assert_eq!(unique_domains.len(), domains.len());
    }

    #[test]
    fn domain_prefixes_are_distinct() {
        let prefixes = PREFIX_PARSERS
            .iter()
            .map(|(prefix, _)| *prefix)
            .collect::<HashSet<_>>();

        assert_eq!(prefixes.len(), PREFIX_PARSERS.len());
    }

    #[test]
    fn hmac_domains_are_distinct() {
        let domains = DOMAIN_CONTRACTS
            .iter()
            .map(|(domain, _)| *domain)
            .collect::<HashSet<_>>();

        assert_eq!(domains.len(), DOMAIN_CONTRACTS.len());
    }

    #[test]
    fn derivation_constants_match_the_recorded_data_contract() {
        for (domain, prefix) in DOMAIN_CONTRACTS {
            let contract_row = format!("| `{domain}` | `{domain}` | `{prefix}` |");

            assert!(
                RECORDED_DATA.contains(&contract_row),
                "recorded-data contract does not contain {contract_row}"
            );
        }
    }

    #[test]
    fn each_domain_accepts_only_its_own_prefix() {
        for (prefix, parses) in PREFIX_PARSERS {
            for (candidate_prefix, _) in PREFIX_PARSERS {
                let value = format!("{candidate_prefix}{TEST_HEX}");

                assert_eq!(
                    parses(&value),
                    prefix == candidate_prefix,
                    "{prefix} domain mishandled {value}"
                );
            }
        }
    }

    #[test]
    fn contract_identifiers_parse_in_their_own_domain() {
        for (prefix, parses) in PREFIX_PARSERS {
            let identifier = contract_identifier(prefix);

            assert!(
                parses(identifier),
                "contract identifier {identifier} does not parse in the {prefix} domain"
            );
        }
    }

    #[test]
    fn identifiers_display_with_their_contract_prefixes() {
        let rendered = [
            SessionId::from_bytes(TEST_BYTES).to_string(),
            RetentionSubject::from_bytes(TEST_BYTES).to_string(),
            AgentSubject::from_bytes(TEST_BYTES).to_string(),
            PackageSubject::from_bytes(TEST_BYTES).to_string(),
            ExtensionSubject::from_bytes(TEST_BYTES).to_string(),
            HookSubject::from_bytes(TEST_BYTES).to_string(),
            PluginSubject::from_bytes(TEST_BYTES).to_string(),
            CommandSubject::from_bytes(TEST_BYTES).to_string(),
        ];

        assert_eq!(
            rendered,
            [
                "sess_00112233445566778899aabbccddeeff",
                "ret_00112233445566778899aabbccddeeff",
                "agt_00112233445566778899aabbccddeeff",
                "pkg_00112233445566778899aabbccddeeff",
                "ext_00112233445566778899aabbccddeeff",
                "hok_00112233445566778899aabbccddeeff",
                "plg_00112233445566778899aabbccddeeff",
                "cmd_00112233445566778899aabbccddeeff",
            ]
        );
    }

    #[test]
    fn identifier_debug_uses_the_wire_form() {
        let identifier = SessionId::from_bytes(TEST_BYTES);

        let debug = format!("{identifier:?}");

        assert_eq!(debug, "sess_00112233445566778899aabbccddeeff");
    }

    #[test]
    fn identifier_text_round_trips() {
        let identifier = SessionId::from_bytes(TEST_BYTES);

        let parsed = identifier.to_string().parse::<SessionId>().unwrap();

        assert_eq!(parsed, identifier);
    }

    #[test]
    fn identifier_json_round_trips() {
        let identifier = SessionId::from_bytes(TEST_BYTES);

        let json = serde_json::to_string(&identifier).unwrap();

        assert_eq!(json, format!("\"sess_{TEST_HEX}\""));
        assert_eq!(
            serde_json::from_str::<SessionId>(&json).unwrap(),
            identifier
        );
    }

    #[test]
    fn identifier_json_rejects_another_domain_prefix() {
        let json = format!("\"cmd_{TEST_HEX}\"");

        let result = serde_json::from_str::<PackageSubject>(&json);

        assert!(result.is_err());
    }

    #[test]
    fn identifier_json_rejects_non_string_values() {
        for json in ["17", "{}", "[]", "true", "null"] {
            let result = serde_json::from_str::<SessionId>(json);

            assert!(result.is_err(), "accepted non-string identifier: {json}");
        }
    }

    #[test]
    fn identifier_rejects_another_domain_prefix() {
        let value = format!("cmd_{TEST_HEX}");

        let result = value.parse::<PackageSubject>();

        assert_eq!(result, Err(ParseScopedIdError::IncorrectPrefix));
    }

    #[test]
    fn identifier_rejects_wrong_lengths() {
        let cases = [
            "sess_",
            "sess_00112233445566778899aabbccddeef",
            "sess_00112233445566778899aabbccddeeff0",
        ];

        for value in cases {
            assert_eq!(
                value.parse::<SessionId>(),
                Err(ParseScopedIdError::IncorrectLength),
                "accepted identifier with the wrong length: {value}"
            );
        }
    }

    #[test]
    fn identifier_rejects_uppercase_hexadecimal() {
        let value = "sess_00112233445566778899AABBCCDDEEFF";

        let result = value.parse::<SessionId>();

        assert_eq!(result, Err(ParseScopedIdError::InvalidHex));
    }

    #[test]
    fn identifier_rejects_non_hexadecimal_character() {
        let value = "sess_00112233445566778899aabbccddeefg";

        let result = value.parse::<SessionId>();

        assert_eq!(result, Err(ParseScopedIdError::InvalidHex));
    }
}
