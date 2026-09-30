//! Declarative helpers for strict versioned telemetry rows.

/// Define a versioned row and its strict, validation-first wire form.
///
/// The field list is the source of truth for the row's declaration, its raw
/// deserialization type, and the mechanical transfer between them. Validation
/// remains an ordinary function in the row module so its rules stay visible.
/// Serde field attributes are applied to both representations. The macro owns
/// container attributes, so callers can supply row documentation but not a
/// second, potentially conflicting set of derives or Serde rules.
macro_rules! strict_versioned_row {
    (
        $(#[doc = $row_doc:literal])*
        $visibility:vis struct $row:ident {
            $(
                $(#[$field_metadata:meta])*
                $field:ident: $field_type:ty,
            )*
        }

        kind: $kind:path,
        raw: $raw:ident,
        error: $error:ty,
        validate: $validate:path,
    ) => {
        $(#[doc = $row_doc])*
        #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
        $visibility struct $row {
            #[serde(rename = "v")]
            version: $crate::telemetry::schema::SchemaVersion,
            kind: $crate::telemetry::schema::RowKind,
            event_id: $crate::telemetry::schema::EventId,
            day: $crate::telemetry::schema::UtcDay,
            $(
                $(#[$field_metadata])*
                $field: $field_type,
            )*
        }

        impl $row {
            const KIND: $crate::telemetry::schema::RowKind = $kind;
        }

        // Serde's `try_from` attribute requires a string literal, which
        // `macro_rules!` cannot construct from `$raw`.
        impl<'de> serde::Deserialize<'de> for $row {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let raw = <$raw as serde::Deserialize>::deserialize(deserializer)?;

                if raw.kind != Self::KIND {
                    return Err(serde::de::Error::custom(format_args!(
                        "expected {:?} row kind, found {:?}",
                        Self::KIND,
                        raw.kind
                    )));
                }

                Self::try_from(raw).map_err(serde::de::Error::custom)
            }
        }

        #[doc = concat!(
            "Strict wire representation validated before becoming `",
            stringify!($row),
            "`."
        )]
        #[derive(Debug, serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $raw {
            #[serde(
                rename = "v",
                deserialize_with = "crate::telemetry::schema::deserialize_version_one"
            )]
            version: $crate::telemetry::schema::SchemaVersion,
            kind: $crate::telemetry::schema::RowKind,
            event_id: $crate::telemetry::schema::EventId,
            day: $crate::telemetry::schema::UtcDay,
            $(
                $(#[$field_metadata])*
                $field: $field_type,
            )*
        }

        impl TryFrom<$raw> for $row {
            type Error = $error;

            fn try_from(raw: $raw) -> Result<Self, Self::Error> {
                $validate(&raw)?;

                Ok(Self {
                    version: raw.version,
                    kind: Self::KIND,
                    event_id: raw.event_id,
                    day: raw.day,
                    $($field: raw.$field,)*
                })
            }
        }
    };
}

pub(super) use strict_versioned_row;
