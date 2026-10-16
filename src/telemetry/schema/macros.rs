//! Declarative helpers for strict versioned telemetry rows.

/// Define a versioned row and its strict wire form.
///
/// The field list is the source of truth for the row's declaration, its raw
/// deserialization type, and the mechanical transfer between them. Rows with
/// cross-field invariants supply a validation function; structurally valid
/// rows omit it. Serde field attributes are applied to both representations.
/// The generated row and raw wire type repeat their common fields because
/// Serde does not combine flattened structs with strict unknown-field
/// rejection.
/// The macro owns container attributes, so callers can supply row
/// documentation but not a second, potentially conflicting set of derives or
/// Serde rules.
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
        $(validate: $validate:path,)?
    ) => {
        $(#[doc = $row_doc])*
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize)]
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

            fn from_raw(raw: $raw) -> Self {
                Self {
                    version: raw.version,
                    kind: Self::KIND,
                    event_id: raw.event_id,
                    day: raw.day,
                    $($field: raw.$field,)*
                }
            }
        }

        impl $crate::telemetry::schema::VersionedRow for $row {
            fn day(&self) -> $crate::telemetry::schema::UtcDay {
                self.day
            }
        }

        // A manual implementation lets the macro name `$raw`; Serde's
        // `try_from` attribute accepts only a string literal.
        impl<'de> ::serde::Deserialize<'de> for $row {
            fn deserialize<D>(deserializer: D) -> ::core::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let raw = <$raw as ::serde::Deserialize>::deserialize(deserializer)?;

                if raw.kind != Self::KIND {
                    return ::core::result::Result::Err(
                        <D::Error as ::serde::de::Error>::custom(format_args!(
                            "expected {:?} row kind, found {:?}",
                            Self::KIND,
                            raw.kind
                        )),
                    );
                }

                $($validate(&raw).map_err(<D::Error as ::serde::de::Error>::custom)?;)?

                ::core::result::Result::Ok(Self::from_raw(raw))
            }
        }

        #[doc = concat!(
            "Strict wire representation validated before becoming `",
            stringify!($row),
            "`."
        )]
        #[derive(Debug, ::serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $raw {
            // Serde requires this callback as a string literal, so it cannot
            // use `$crate`. This crate-relative path is safe while the macro
            // remains crate-private.
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

    };
}

pub(super) use strict_versioned_row;

#[cfg(test)]
mod tests {
    mod call_site_with_shadowed_names {
        use crate::telemetry::schema::RowKind;

        type Result<T> = ::core::result::Result<T, ()>;

        mod serde {
            pub(super) struct Shadow;
        }

        strict_versioned_row! {
            struct ShadowedNamesV1 {}

            kind: RowKind::StorageLimit,
            raw: RawShadowedNamesV1,
        }

        #[test]
        fn generated_paths_ignore_call_site_shadowing() {
            let _: Result<()> = ::core::result::Result::Ok(());
            let _shadow = serde::Shadow;
            let json = concat!(
                r#"{"v":1,"kind":"storage_limit","#,
                r#""event_id":"00000000-0000-4000-8000-000000000000","#,
                r#""day":"2026-08-03"}"#,
            );

            let decoded = ::serde_json::from_str::<ShadowedNamesV1>(json).unwrap();
            let encoded = ::serde_json::to_value(decoded).unwrap();
            let expected = ::serde_json::from_str::<::serde_json::Value>(json).unwrap();

            assert_eq!(encoded, expected);
        }
    }
}
