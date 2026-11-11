//! The audits file format.
//!
//! The audits list carries `audits/<pm>/<name>.toml` files: one per package,
//! saying which payloads of that package have been audited and which releases
//! or payloads must never load. For a crate, `<pm>` is `cargo` and `<name>` is
//! the crate's name as published on crates.io, lowercase, hyphens kept. The
//! file names the package by its path, so it has no field for it.
//!
//! This module is the schema (the `Raw*` types, in the style of
//! [`crate::manifest`]) and the [`Digest`] type that names a payload. They live
//! in the SDK so that tooling outside Symposium, such as the validator and
//! runner that maintain a list, speaks exactly the format the client reads.
//!
//! The types are *unvalidated*: they say what a file can contain, not what it
//! means. Reading them leniently (dropping a malformed entry, ignoring a file
//! of an unknown [`FORMAT`]) is the client's policy, not the schema's, so none
//! of the types deny unknown fields.
//!
//! ```toml
//! # audits/cargo/widget-lib.toml
//! format = 1
//!
//! [[audit]]
//! digest   = "sha256:<64 hex>"
//! versions = ["1.4.2", "1.4.3"]   # bookkeeping; clients never match on it
//!
//! [[revoked]]
//! versions = ">=1.3.0, <1.4.2"    # exactly one of `versions` or `digest`
//! reason   = "remote-exec"
//! ```

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The only `format` value this version of the SDK understands.
pub const FORMAT: u32 = 1;

/// The directory at the root of the audits list that holds the audits files.
pub const AUDITS_DIR: &str = "audits";

/// The contents of one `audits/<pm>/<name>.toml` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawAuditFile {
    /// Schema version. Clients ignore a whole file whose value they do not
    /// know.
    pub format: u32,
    /// Payloads that have been audited.
    #[serde(default, rename = "audit", skip_serializing_if = "Vec::is_empty")]
    pub audits: Vec<RawAudit>,
    /// Releases or payloads that must not load.
    #[serde(default, rename = "revoked", skip_serializing_if = "Vec::is_empty")]
    pub revoked: Vec<RawRevoked>,
}

/// One audited payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawAudit {
    /// The payload digest, `sha256:<64 hex>`. A string here so that a client
    /// can drop an entry with a bad digest instead of failing the file.
    pub digest: String,
    /// Releases known to carry this payload, as exact versions. Bookkeeping
    /// for people and for the maintaining runner; the client never matches on
    /// it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<String>,
}

/// One revocation. Exactly one of `versions` and `digest` must be set; an
/// entry with both or neither is malformed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawRevoked {
    /// A semver requirement (`>=1.3.0, <1.4.2`) over package versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<String>,
    /// A payload digest, `sha256:<64 hex>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Why, as a code. Required: an entry without one is malformed. A code
    /// this version does not know reads as [`RevokeReason::Unknown`] and still
    /// revokes, so a list that adds a code never fails open on older clients.
    pub reason: RevokeReason,
}

/// Why a release or payload was revoked: a code, never prose, so the client
/// renders the sentence itself (see [`RevokeReason::sentence`]). Each variant's
/// doc is its sentence.
///
/// The list may add codes, so the enum is non-exhaustive and every code this
/// version does not know reads as [`RevokeReason::Unknown`]: a revocation must
/// never be dropped because its reason is newer than the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RevokeReason {
    /// Tells the agent to fetch and run remote code.
    RemoteExec,
    /// Tries to send secrets or source to a third party.
    Exfiltration,
    /// Hides what it does from the user.
    HiddenBehavior,
    /// Steers the agent away from the user's task.
    OffTask,
    /// Comes from a crate marked as malicious.
    MaliciousCrate,
    /// A code this version does not know. It still revokes.
    #[serde(other)]
    Unknown,
}

impl RevokeReason {
    /// The reason as one sentence fragment, for `status` output: "it
    /// <sentence>".
    pub fn sentence(self) -> &'static str {
        match self {
            Self::RemoteExec => "tells the agent to fetch and run remote code",
            Self::Exfiltration => "tries to send secrets or source to a third party",
            Self::HiddenBehavior => "hides what it does from the user",
            Self::OffTask => "steers the agent away from the user's task",
            Self::MaliciousCrate => "comes from a crate marked as malicious",
            Self::Unknown => "has a reason code this version of symposium does not know",
        }
    }
}

/// A payload digest: SHA-256 over the framed payload, spelled
/// `sha256:<64 lowercase hex>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The prefix every digest string carries.
    pub const PREFIX: &'static str = "sha256:";

    /// A digest from the raw hash bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw hash bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(Self::PREFIX)?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

/// Why a string is not a [`Digest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDigestError(String);

impl fmt::Display for ParseDigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseDigestError {}

impl FromStr for Digest {
    type Err = ParseDigestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix(Self::PREFIX).ok_or_else(|| {
            ParseDigestError(format!("digest `{s}` does not start with `sha256:`"))
        })?;
        if hex.len() != 64 {
            return Err(ParseDigestError(format!(
                "digest `{s}` must be 64 hex characters after `sha256:`"
            )));
        }
        let mut bytes = [0u8; 32];
        for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
            let nibble = |c: u8| match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'a'..=b'f' => Some(c - b'a' + 10),
                _ => None,
            };
            match (nibble(pair[0]), nibble(pair[1])) {
                (Some(hi), Some(lo)) => bytes[i] = hi << 4 | lo,
                _ => {
                    return Err(ParseDigestError(format!(
                        "digest `{s}` must be lowercase hex"
                    )));
                }
            }
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn digest_round_trips_through_its_string_and_rejects_malformed_ones() {
        let text = format!("sha256:{HEX}");
        let digest: Digest = text.parse().unwrap();
        assert_eq!(digest.to_string(), text);
        assert_eq!(digest.as_bytes()[0], 0x01);
        for bad in [
            HEX.to_string(),
            format!("sha1:{HEX}"),
            "sha256:abcd".to_string(),
            format!("sha256:{}", HEX.to_uppercase()),
            format!("sha256:{}g", &HEX[1..]),
        ] {
            assert!(bad.parse::<Digest>().is_err(), "{bad}");
        }
    }

    #[test]
    fn file_parses_and_serializes() {
        let text = format!(
            r#"
format = 1

[[audit]]
digest = "sha256:{HEX}"
versions = ["1.4.2", "1.4.3"]

[[revoked]]
versions = ">=1.3.0, <1.4.2"
reason = "remote-exec"

[[revoked]]
digest = "sha256:{HEX}"
reason = "hidden-behavior"
"#
        );
        let file: RawAuditFile = toml::from_str(&text).unwrap();
        assert_eq!(file.format, FORMAT);
        assert_eq!(file.audits[0].versions, ["1.4.2", "1.4.3"]);
        assert_eq!(file.revoked[0].reason, RevokeReason::RemoteExec);
        assert_eq!(file.revoked[1].reason, RevokeReason::HiddenBehavior);
        let again: RawAuditFile = toml::from_str(&toml::to_string(&file).unwrap()).unwrap();
        assert_eq!(again, file);
    }

    /// A reason is required, and a code this version does not know still
    /// revokes.
    #[test]
    fn an_unknown_reason_still_revokes() {
        let raw: RawRevoked =
            toml::from_str("versions = \"<1.0.0\"\nreason = \"typosquat\"\n").unwrap();
        assert_eq!(raw.reason, RevokeReason::Unknown);
        assert!(!raw.reason.sentence().is_empty());
        assert!(toml::from_str::<RawRevoked>("versions = \"<1.0.0\"\n").is_err());
    }
}
