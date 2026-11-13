//! The credential trials reach the model with: an API key or a subscription token from
//! the runner's environment, which `srt` masks so no trial can read it.

use std::ffi::OsString;

use thiserror::Error;

use crate::run::ApiKey;

/// The variables Claude Code reads a credential from. Exactly one must be set, so the
/// runner never guesses which the developer meant.
const CLAUDE_CREDENTIALS: [&str; 2] = ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"];

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum CredentialError {
    #[error("set {} or {} for Claude Code", CLAUDE_CREDENTIALS[0], CLAUDE_CREDENTIALS[1])]
    Missing,
    #[error("both {} and {} are set; unset one", CLAUDE_CREDENTIALS[0], CLAUDE_CREDENTIALS[1])]
    Ambiguous,
}

/// The one credential `var` holds for Claude Code. An empty variable counts as unset.
pub(crate) fn claude(var: impl Fn(&str) -> Option<OsString>) -> Result<ApiKey, CredentialError> {
    let mut set = CLAUDE_CREDENTIALS.into_iter().filter_map(|name| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(|value| (name, value))
    });
    match (set.next(), set.next()) {
        (Some((name, value)), None) => Ok(ApiKey::new(name, value)),
        (None, _) => Err(CredentialError::Missing),
        (Some(_), Some(_)) => Err(CredentialError::Ambiguous),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential_from(vars: &[(&str, &str)]) -> Result<ApiKey, CredentialError> {
        claude(|name| {
            vars.iter()
                .find(|(set, _)| *set == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    #[test]
    fn exactly_one_nonempty_credential_is_used() {
        for name in CLAUDE_CREDENTIALS {
            let credential = credential_from(&[(name, "secret")]).unwrap();

            assert_eq!(credential.name(), name);
        }
    }

    #[test]
    fn an_absent_or_empty_credential_is_missing() {
        for vars in [&[][..], &[("ANTHROPIC_API_KEY", "")][..]] {
            let error = credential_from(vars).unwrap_err();

            assert_eq!(error, CredentialError::Missing, "{vars:?}");
        }
    }

    #[test]
    fn two_credentials_are_ambiguous() {
        let error = credential_from(&[
            ("ANTHROPIC_API_KEY", "key"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "token"),
        ])
        .unwrap_err();

        assert_eq!(error, CredentialError::Ambiguous);
    }
}
