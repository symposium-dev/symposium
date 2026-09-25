//! Validation shared by public names in the telemetry contract.

/// Define a public-name newtype and its grammar-specific error type.
macro_rules! validated_string_newtype {
    (
        $(#[$metadata:meta])*
        $visibility:vis struct $name:ident {
            error = $error:ident;
            maximum_bytes = $maximum_bytes:expr;
            initial_byte_rule = $initial_byte_rule:expr;
            invalid_initial = $invalid_initial:ident;
            noun = $noun:literal;
            as_str_doc = $as_str_doc:literal;
        }
    ) => {
        $(#[$metadata])*
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(try_from = "String")]
        $visibility struct $name(String);

        impl $name {
            #[doc = $as_str_doc]
            #[must_use]
            $visibility fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = $error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                $crate::telemetry::schema::name::validate_public_name(
                    &value,
                    $maximum_bytes,
                    $initial_byte_rule,
                )
                .map_err($error::from)?;
                Ok(Self(value))
            }
        }

        impl std::str::FromStr for $name {
            type Err = $error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                $crate::telemetry::schema::name::validate_public_name(
                    value,
                    $maximum_bytes,
                    $initial_byte_rule,
                )
                .map_err($error::from)?;
                Ok(Self(value.to_owned()))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        #[doc = concat!("Reason a ", $noun, " cannot enter public telemetry.")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $visibility enum $error {
            Empty,
            TooLong,
            $invalid_initial,
            UnsupportedCharacter,
        }

        impl From<$crate::telemetry::schema::name::PublicNameViolation> for $error {
            fn from(
                violation: $crate::telemetry::schema::name::PublicNameViolation,
            ) -> Self {
                match violation {
                    $crate::telemetry::schema::name::PublicNameViolation::Empty => Self::Empty,
                    $crate::telemetry::schema::name::PublicNameViolation::TooLong => Self::TooLong,
                    $crate::telemetry::schema::name::PublicNameViolation::InvalidInitialByte => {
                        Self::$invalid_initial
                    }
                    $crate::telemetry::schema::name::PublicNameViolation::UnsupportedCharacter => {
                        Self::UnsupportedCharacter
                    }
                }
            }
        }

        impl std::fmt::Display for $error {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Empty => write!(formatter, "{} must not be empty", $noun),
                    Self::TooLong => write!(
                        formatter,
                        "{} exceeds {} bytes",
                        $noun,
                        $maximum_bytes,
                    ),
                    Self::$invalid_initial => write!(
                        formatter,
                        "{} must start with {}",
                        $noun,
                        ($initial_byte_rule).description(),
                    ),
                    Self::UnsupportedCharacter => write!(
                        formatter,
                        "{} may contain only ASCII letters, digits, hyphens, and underscores",
                        $noun,
                    ),
                }
            }
        }

        impl std::error::Error for $error {}
    };
}

pub(super) use validated_string_newtype;

/// Rule applied to the first byte of a public name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InitialByteRule {
    Alphabetic,
    Alphanumeric,
}

impl InitialByteRule {
    /// Describe the initial byte accepted by this validation rule.
    pub(super) const fn description(self) -> &'static str {
        match self {
            Self::Alphabetic => "an ASCII letter",
            Self::Alphanumeric => "an ASCII letter or digit",
        }
    }
}

/// Structural reason a public name fails its versioned grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicNameViolation {
    Empty,
    TooLong,
    InvalidInitialByte,
    UnsupportedCharacter,
}

/// Validate the common ASCII shape of a versioned public telemetry name.
pub(super) fn validate_public_name(
    value: &str,
    maximum_bytes: usize,
    initial_byte_rule: InitialByteRule,
) -> Result<(), PublicNameViolation> {
    let Some((first, rest)) = value.as_bytes().split_first() else {
        return Err(PublicNameViolation::Empty);
    };

    if value.len() > maximum_bytes {
        return Err(PublicNameViolation::TooLong);
    }

    let initial_byte_is_valid = match initial_byte_rule {
        InitialByteRule::Alphabetic => first.is_ascii_alphabetic(),
        InitialByteRule::Alphanumeric => first.is_ascii_alphanumeric(),
    };
    if !initial_byte_is_valid {
        return Err(PublicNameViolation::InvalidInitialByte);
    }

    if !rest
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PublicNameViolation::UnsupportedCharacter);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_byte_rules_describe_their_enforced_grammar() {
        assert_eq!(InitialByteRule::Alphabetic.description(), "an ASCII letter");
        assert_eq!(
            InitialByteRule::Alphanumeric.description(),
            "an ASCII letter or digit"
        );
    }
}
