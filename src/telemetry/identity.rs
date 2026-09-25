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

/// Identity material bound to one canonical rotation window.
///
/// Private telemetry state creates this handle after applying lifecycle
/// transitions. Subject-bearing row constructors can use it to derive an
/// identifier from the corresponding source value instead of accepting the two
/// independently.
pub(super) struct IdentityScope<'a, A> {
    key: &'a IdentityKey,
    window: String,
    anchor: PhantomData<A>,
}

impl<'a, A> IdentityScope<'a, A> {
    /// Bind a private identity key to one canonical window value.
    ///
    /// In production, `window` comes from a validated state anchor. Keeping the
    /// conversion here avoids making the identity module depend on row schema
    /// types.
    #[must_use]
    pub(super) fn new(key: &'a IdentityKey, window: String) -> Self {
        Self {
            key,
            window,
            anchor: PhantomData,
        }
    }

    /// Derive the identifier belonging to a typed source value.
    #[must_use]
    pub(super) fn derive<I>(&self, dimension: &I) -> ScopedId<I::Domain>
    where
        I: IdentityDimension,
        I::Domain: ScopedIdDomain<Anchor = A>,
    {
        derive_scoped_id(self.key, self.window.as_bytes(), dimension)
    }
}

/// Identity material bound to the active 30-day identifier window.
pub(super) type IdentifierWindowScope<'a> = IdentityScope<'a, IdentifierWindowAnchor>;

/// Identity material bound to the active D0-D30 return cohort.
pub(super) type ReturnCohortScope<'a> = IdentityScope<'a, ReturnCohortAnchor>;

/// A typed value that supplies one identifier domain's canonical fields.
///
/// Each schema type implements this trait for the domain it belongs to. This
/// keeps field selection and order beside the validated value while leaving
/// framing under the identity module's control. A domain with no schema value,
/// such as retention, keeps its empty dimension here so private state can use
/// the production encoding without depending on a row module.
pub(super) trait IdentityDimension {
    type Domain;

    /// Write this dimension's fields in their frozen contract order.
    ///
    /// Use [`DimensionWriter::variant`] for tagged values and
    /// [`DimensionWriter::sequence`] for counted collections. Implementations
    /// must not add their own framing.
    fn write(&self, writer: &mut DimensionWriter<'_>);
}

/// The empty dimension used to derive a return-cohort subject.
///
/// A retention subject is scoped only by its return-cohort anchor. Keeping the
/// empty dimension as a type ensures callers cannot add an accidental field to
/// that derivation.
pub(super) struct RetentionDimension;

impl IdentityDimension for RetentionDimension {
    type Domain = RetentionDomain;

    /// Write no fields, as required by the version 1 identity contract.
    fn write(&self, _writer: &mut DimensionWriter<'_>) {}
}

/// Writes canonical identity-dimension framing to a private byte sink.
///
/// Only this module can create a writer. Schema types can use its structured
/// operations from an [`IdentityDimension`] implementation, but telemetry
/// producers cannot construct dimensions from loose byte slices.
pub(super) struct DimensionWriter<'a> {
    write: &'a mut dyn FnMut(&[u8]),
}

impl<'a> DimensionWriter<'a> {
    fn new(write: &'a mut dyn FnMut(&[u8])) -> Self {
        Self { write }
    }

    /// Write one length-prefixed field.
    pub(super) fn field(&mut self, value: &[u8]) {
        write_frame(value, |bytes| (self.write)(bytes));
    }

    /// Write a tagged variant followed by its canonically framed fields.
    ///
    /// Variant labels are frozen contract values, so callers supply a static
    /// string rather than data obtained at runtime.
    pub(super) fn variant(&mut self, label: &'static str, write_fields: impl FnOnce(&mut Self)) {
        self.field(label.as_bytes());
        write_fields(self);
    }

    /// Write a counted sequence whose items own their recursive encoding.
    pub(super) fn sequence<T>(&mut self, items: &[T], mut write_item: impl FnMut(&mut Self, &T)) {
        let count = u64::try_from(items.len())
            .expect("BUG: a slice length must fit the telemetry sequence format");
        (self.write)(&count.to_be_bytes());

        for item in items {
            write_item(self, item);
        }
    }
}

#[cfg(test)]
pub(super) fn encode_dimension_for_test(dimension: &impl IdentityDimension) -> Vec<u8> {
    let mut encoded = Vec::new();
    let mut append = |bytes: &[u8]| encoded.extend_from_slice(bytes);
    let mut writer = DimensionWriter::new(&mut append);
    dimension.write(&mut writer);
    encoded
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

mod sealed {
    pub trait Sealed {}
}

/// Marker for the state anchor that scopes an identifier domain.
pub(super) trait IdentityAnchor: sealed::Sealed {
    const CONTRACT_NAME: &'static str;
}

/// The anchor shared by identifiers that rotate on the 30-day window.
pub(super) enum IdentifierWindowAnchor {}

impl sealed::Sealed for IdentifierWindowAnchor {}

impl IdentityAnchor for IdentifierWindowAnchor {
    const CONTRACT_NAME: &'static str = "identifier-window";
}

/// The anchor dedicated to D0-D30 return measurement.
pub(super) enum ReturnCohortAnchor {}

impl sealed::Sealed for ReturnCohortAnchor {}

impl IdentityAnchor for ReturnCohortAnchor {
    const CONTRACT_NAME: &'static str = "return-cohort";
}

/// Marker supplying a [`ScopedId`] domain's frozen derivation and wire labels.
///
/// Visible only inside telemetry so typed derivation APIs can name the bound.
/// The real domains remain declared centrally below: these constants are
/// published contract surfaces, not general extension points. Changing an
/// anchor category or either string requires a new consent version.
pub(super) trait ScopedIdDomain: sealed::Sealed {
    type Anchor: IdentityAnchor;

    const PREFIX: &'static str;
    const HMAC_DOMAIN: &'static str;
}

fn derive_scoped_id<I>(key: &IdentityKey, window: &[u8], dimension: &I) -> ScopedId<I::Domain>
where
    I: IdentityDimension,
    I::Domain: ScopedIdDomain,
{
    let mut hmac =
        HmacSha256::new_from_slice(&key.0).expect("BUG: HMAC-SHA-256 must accept a 32-byte key");
    hmac.update(b"telemetry:");
    hmac.update(I::Domain::HMAC_DOMAIN.as_bytes());
    hmac.update(b":v1\0");
    write_frame(window, |bytes| hmac.update(bytes));
    let mut update = |bytes: &[u8]| hmac.update(bytes);
    let mut writer = DimensionWriter::new(&mut update);
    dimension.write(&mut writer);

    let digest = hmac.finalize().into_bytes();
    let mut bytes = [0; IDENTIFIER_BYTES];
    bytes.copy_from_slice(&digest[..IDENTIFIER_BYTES]);
    ScopedId::from_bytes(bytes)
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

/// Declares each identifier domain, its anchor, contract strings, and alias.
///
/// A macro because a function cannot introduce types, and the prefix is the
/// hand-written part worth handing to the tests as a table.
macro_rules! scoped_id_domains {
    (
        $(
            $domain:ident => $alias:ident {
                anchor: $anchor:ty,
                hmac_domain: $hmac_domain:literal,
                wire_prefix: $prefix:literal,
            }
        )+
    ) => {
        $(
            pub(super) enum $domain {}

            impl sealed::Sealed for $domain {}

            impl ScopedIdDomain for $domain {
                type Anchor = $anchor;

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
        const DOMAIN_CONTRACTS: &[(&str, &str, &str)] = &[
            $(($hmac_domain, $prefix, <$anchor as IdentityAnchor>::CONTRACT_NAME)),+
        ];
    };
}

scoped_id_domains! {
    SessionDomain => SessionId {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "session_id",
        wire_prefix: "sess_",
    }
    RetentionDomain => RetentionSubject {
        anchor: ReturnCohortAnchor,
        hmac_domain: "retention_subject",
        wire_prefix: "ret_",
    }
    AgentDomain => AgentSubject {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "agent_subject",
        wire_prefix: "agt_",
    }
    PackageDomain => PackageSubject {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "package_subject",
        wire_prefix: "pkg_",
    }
    ExtensionDomain => ExtensionSubject {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "extension_subject",
        wire_prefix: "ext_",
    }
    HookDomain => HookSubject {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "hook_subject",
        wire_prefix: "hok_",
    }
    PluginDomain => PluginSubject {
        anchor: IdentifierWindowAnchor,
        hmac_domain: "plugin_subject",
        wire_prefix: "plg_",
    }
    CommandDomain => CommandSubject {
        anchor: IdentifierWindowAnchor,
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

    impl sealed::Sealed for TestDomain {}

    impl ScopedIdDomain for TestDomain {
        type Anchor = IdentifierWindowAnchor;

        const PREFIX: &'static str = "test_";
        const HMAC_DOMAIN: &'static str = "test_subject";
    }

    struct TestDimension<D> {
        fields: Vec<&'static [u8]>,
        domain: PhantomData<D>,
    }

    impl<D> TestDimension<D> {
        fn from_fields<const N: usize>(fields: [&'static [u8]; N]) -> Self {
            Self {
                fields: fields.into_iter().collect(),
                domain: PhantomData,
            }
        }
    }

    impl<D> IdentityDimension for TestDimension<D> {
        type Domain = D;

        fn write(&self, writer: &mut DimensionWriter<'_>) {
            for field in &self.fields {
                writer.field(field);
            }
        }
    }

    struct TestVariantDimension;

    impl IdentityDimension for TestVariantDimension {
        type Domain = TestDomain;

        fn write(&self, writer: &mut DimensionWriter<'_>) {
            writer.variant("package", |writer| writer.field(b"cargo"));
        }
    }

    struct TestNestedSequenceDimension;

    impl IdentityDimension for TestNestedSequenceDimension {
        type Domain = TestDomain;

        fn write(&self, writer: &mut DimensionWriter<'_>) {
            let groups = [
                [b"a".as_slice(), b"bc".as_slice()],
                [b"d".as_slice(), b"ef".as_slice()],
            ];

            writer.sequence(&groups, |writer, group| {
                writer.sequence(group, |writer, item| writer.field(item));
            });
        }
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
        let identity = IdentifierWindowScope::new(&key, "window-1".to_owned());
        let dimension = TestDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);

        let identifier = identity.derive(&dimension);

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
        let identity = IdentifierWindowScope::new(&key, "window-1".to_owned());
        let dimension = TestDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);

        let first = identity.derive(&dimension);
        let second = identity.derive(&dimension);

        assert_eq!(first, second);
    }

    #[test]
    fn return_cohort_scope_derives_retention_subjects() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let identity = ReturnCohortScope::new(&key, "2026-08-03".to_owned());

        let identifier = identity.derive(&RetentionDimension);

        // Cross-checked with .NET's HMACSHA256 over the retention header and
        // framed cohort anchor. The complete digest is
        // 270adecd2120c543261f04bd771df49170e407de5d3116f98a0468f832fcfcbb.
        assert_eq!(
            identifier.to_string(),
            "ret_270adecd2120c543261f04bd771df491"
        );
    }

    #[test]
    fn length_framing_separates_nul_at_different_boundaries() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let first_identity = IdentifierWindowScope::new(&key, "a\0b".to_owned());
        let first_dimension = TestDimension::<SessionDomain>::from_fields([b"c".as_slice()]);
        let second_identity = IdentifierWindowScope::new(&key, "a".to_owned());
        let second_dimension = TestDimension::<SessionDomain>::from_fields([b"b\0c".as_slice()]);

        let first = first_identity.derive(&first_dimension);
        let second = second_identity.derive(&second_dimension);

        assert_ne!(first, second);
    }

    #[test]
    fn length_framing_separates_dimension_field_boundaries() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let identity = IdentifierWindowScope::new(&key, "window-1".to_owned());
        let first_dimension = TestDimension::<PackageDomain>::from_fields([
            b"cargo".as_slice(),
            b"foo1".as_slice(),
            b"2.3.4".as_slice(),
        ]);
        let second_dimension = TestDimension::<PackageDomain>::from_fields([
            b"cargo".as_slice(),
            b"foo".as_slice(),
            b"12.3.4".as_slice(),
        ]);

        let first = identity.derive(&first_dimension);
        let second = identity.derive(&second_dimension);

        assert_ne!(first, second);
    }

    #[test]
    fn dimension_writer_prefixes_variant_fields_with_their_label() {
        let variant_length = 7_u64.to_be_bytes();
        let field_length = 5_u64.to_be_bytes();
        let expected = [
            variant_length.as_slice(),
            b"package".as_slice(),
            field_length.as_slice(),
            b"cargo".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&TestVariantDimension);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn dimension_writer_encodes_counted_sequences_recursively() {
        let item_count = 2_u64.to_be_bytes();
        let one_byte = 1_u64.to_be_bytes();
        let two_bytes = 2_u64.to_be_bytes();
        let expected = [
            item_count.as_slice(),
            item_count.as_slice(),
            one_byte.as_slice(),
            b"a".as_slice(),
            two_bytes.as_slice(),
            b"bc".as_slice(),
            item_count.as_slice(),
            one_byte.as_slice(),
            b"d".as_slice(),
            two_bytes.as_slice(),
            b"ef".as_slice(),
        ]
        .concat();

        let encoded = encode_dimension_for_test(&TestNestedSequenceDimension);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn changing_any_derivation_scope_changes_the_identifier() {
        let key = IdentityKey::from_bytes([0x42; IDENTITY_KEY_BYTES]);
        let other_key = IdentityKey::from_bytes([0x24; IDENTITY_KEY_BYTES]);
        let identity = IdentifierWindowScope::new(&key, "window-1".to_owned());
        let other_key_identity = IdentifierWindowScope::new(&other_key, "window-1".to_owned());
        let other_window_identity = IdentifierWindowScope::new(&key, "window-2".to_owned());
        let session_dimension =
            TestDimension::<SessionDomain>::from_fields([b"dimension-1".as_slice()]);
        let other_session_dimension =
            TestDimension::<SessionDomain>::from_fields([b"dimension-2".as_slice()]);
        let command_dimension =
            TestDimension::<CommandDomain>::from_fields([b"dimension-1".as_slice()]);

        // Different domain markers produce different identifier types, so use
        // their shared byte representation for this one collection.
        let identifiers = [
            identity.derive(&session_dimension).bytes,
            other_key_identity.derive(&session_dimension).bytes,
            identity.derive(&command_dimension).bytes,
            other_window_identity.derive(&session_dimension).bytes,
            identity.derive(&other_session_dimension).bytes,
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
            .map(|(domain, _, _)| *domain)
            .collect::<HashSet<_>>();

        assert_eq!(domains.len(), DOMAIN_CONTRACTS.len());
    }

    #[test]
    fn derivation_constants_match_the_recorded_data_contract() {
        for (domain, prefix, anchor) in DOMAIN_CONTRACTS {
            let contract_row = format!("| `{domain}` | `{domain}` | `{prefix}` | `{anchor}` |");

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
