//! Discovery policy evaluation.
//!
//! Implements the allow/deny specificity rules for workspace dependency
//! auto-discovery. Candidates are matched against collected policy rules
//! (from user config and installed plugin manifests), and only allowed
//! candidates are resolved as plugin sources.

use crate::config::{
    DiscoveryPolicy, DiscoveryRegistryRules, DiscoveryRules, RegistryDiscoveryRule,
};
use crate::crate_sources::normalize_crate_name;

/// A candidate plugin source produced by a registry scan (e.g. a workspace
/// dependency that might have a `SYMPOSIUM.toml`).
#[derive(Debug, Clone)]
pub enum DiscoveryCandidate {
    /// A crate-registry candidate (workspace dependency).
    Crate {
        name: String,
        #[allow(dead_code)]
        version: String,
    },
}

/// Collected discovery policy from all sources.
#[derive(Debug, Clone, Default)]
pub struct CollectedPolicy {
    rules: Vec<PolicyEntry>,
}

#[derive(Debug, Clone)]
struct PolicyEntry {
    allow: bool,
    matcher: PolicyMatcher,
}

#[derive(Debug, Clone)]
enum PolicyMatcher {
    /// Matches everything (`discovery.allow = "*"` or `discovery.deny = "*"`).
    Any,
    /// Matches all crate-registry candidates (`discovery.allow.crates = "*"`).
    AnyCrate,
    /// Matches a specific crate by name with a version pattern.
    Crate { name: String, version: String },
}

impl PolicyMatcher {
    fn specificity(&self) -> u8 {
        match self {
            PolicyMatcher::Any => 0,
            PolicyMatcher::AnyCrate => 1,
            PolicyMatcher::Crate { .. } => 2,
        }
    }

    fn matches(&self, candidate: &DiscoveryCandidate) -> bool {
        match (self, candidate) {
            (PolicyMatcher::Any, _) => true,
            (PolicyMatcher::AnyCrate, DiscoveryCandidate::Crate { .. }) => true,
            (
                PolicyMatcher::Crate { name, version },
                DiscoveryCandidate::Crate {
                    name: cand_name, ..
                },
            ) => {
                let normalized_rule = normalize_crate_name(name);
                let normalized_cand = normalize_crate_name(cand_name);
                normalized_rule == normalized_cand && (version == "*" || version == cand_name)
            }
        }
    }
}

/// The verdict from evaluating a candidate against the collected policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Allowed by a matching rule.
    Allowed,
    /// Denied by a matching rule or by default (no rule matched).
    Denied,
}

impl CollectedPolicy {
    /// Add rules from a `DiscoveryPolicy` (user config or plugin manifest).
    pub fn add_policy(&mut self, policy: &DiscoveryPolicy) {
        self.add_rules(&policy.allow, true);
        self.add_rules(&policy.deny, false);
    }

    fn add_rules(&mut self, rules: &DiscoveryRules, allow: bool) {
        match rules {
            DiscoveryRules::Empty => {}
            DiscoveryRules::Any => {
                self.rules.push(PolicyEntry {
                    allow,
                    matcher: PolicyMatcher::Any,
                });
            }
            DiscoveryRules::Registries(reg) => {
                self.add_registry_rules(reg, allow);
            }
        }
    }

    fn add_registry_rules(&mut self, rules: &DiscoveryRegistryRules, allow: bool) {
        match &rules.crates {
            RegistryDiscoveryRule::Empty => {}
            RegistryDiscoveryRule::Any => {
                self.rules.push(PolicyEntry {
                    allow,
                    matcher: PolicyMatcher::AnyCrate,
                });
            }
            RegistryDiscoveryRule::Specs(specs) => {
                for (name, version) in specs {
                    self.rules.push(PolicyEntry {
                        allow,
                        matcher: PolicyMatcher::Crate {
                            name: name.clone(),
                            version: version.clone(),
                        },
                    });
                }
            }
        }
        // Path and git rules are not yet evaluated as candidates; reserved
        // for future registry expansion.
    }

    /// Evaluate a candidate against the collected policy.
    ///
    /// Rules:
    /// 1. The most specific matching rule wins.
    /// 2. If allow and deny have the same specificity, deny wins.
    /// 3. If no rule matches, the default is deny.
    pub fn evaluate(&self, candidate: &DiscoveryCandidate) -> PolicyVerdict {
        let mut best_specificity: Option<u8> = None;
        let mut best_allow = false;

        for entry in &self.rules {
            if !entry.matcher.matches(candidate) {
                continue;
            }
            let spec = entry.matcher.specificity();
            match best_specificity {
                None => {
                    best_specificity = Some(spec);
                    best_allow = entry.allow;
                }
                Some(current) => {
                    if spec > current {
                        best_specificity = Some(spec);
                        best_allow = entry.allow;
                    } else if spec == current && !entry.allow {
                        // Same specificity: deny wins.
                        best_allow = false;
                    }
                }
            }
        }

        match best_specificity {
            Some(_) if best_allow => PolicyVerdict::Allowed,
            _ => PolicyVerdict::Denied,
        }
    }

    /// Returns true if the policy has any allow rules at all.
    pub fn has_any_allow_rules(&self) -> bool {
        self.rules.iter().any(|r| r.allow)
    }

    /// Number of rules in the policy (used to detect when new rules are added).
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crate_candidate(name: &str) -> DiscoveryCandidate {
        DiscoveryCandidate::Crate {
            name: name.to_string(),
            version: "1.0.0".to_string(),
        }
    }

    fn policy_from_toml(toml_str: &str) -> CollectedPolicy {
        let policy: DiscoveryPolicy = toml::from_str(toml_str).unwrap();
        let mut collected = CollectedPolicy::default();
        collected.add_policy(&policy);
        collected
    }

    #[test]
    fn default_policy_denies_everything() {
        let policy = CollectedPolicy::default();
        assert_eq!(
            policy.evaluate(&crate_candidate("foo")),
            PolicyVerdict::Denied
        );
    }

    #[test]
    fn wildcard_allow_permits_all() {
        let policy = policy_from_toml(r#"allow = "*""#);
        assert_eq!(
            policy.evaluate(&crate_candidate("anything")),
            PolicyVerdict::Allowed
        );
    }

    #[test]
    fn crates_wildcard_allows_crate_candidates() {
        let policy = policy_from_toml(
            r#"
            [allow]
            crates = "*"
            "#,
        );
        assert_eq!(
            policy.evaluate(&crate_candidate("serde")),
            PolicyVerdict::Allowed
        );
    }

    #[test]
    fn specific_allow_overrides_wildcard_deny() {
        let mut collected = CollectedPolicy::default();
        let deny_all: DiscoveryPolicy = toml::from_str(r#"deny = "*""#).unwrap();
        let allow_specific: DiscoveryPolicy = toml::from_str(
            r#"
            [allow]
            crates = { my-crate = "*" }
            "#,
        )
        .unwrap();
        collected.add_policy(&deny_all);
        collected.add_policy(&allow_specific);

        assert_eq!(
            collected.evaluate(&crate_candidate("my-crate")),
            PolicyVerdict::Allowed
        );
        assert_eq!(
            collected.evaluate(&crate_candidate("other")),
            PolicyVerdict::Denied
        );
    }

    #[test]
    fn specific_deny_beats_wildcard_allow() {
        let mut collected = CollectedPolicy::default();
        let allow_all: DiscoveryPolicy = toml::from_str(r#"allow = "*""#).unwrap();
        let deny_specific: DiscoveryPolicy = toml::from_str(
            r#"
            [deny]
            crates = { bad-crate = "*" }
            "#,
        )
        .unwrap();
        collected.add_policy(&allow_all);
        collected.add_policy(&deny_specific);

        assert_eq!(
            collected.evaluate(&crate_candidate("bad-crate")),
            PolicyVerdict::Denied
        );
        assert_eq!(
            collected.evaluate(&crate_candidate("good-crate")),
            PolicyVerdict::Allowed
        );
    }

    #[test]
    fn same_specificity_deny_wins() {
        let mut collected = CollectedPolicy::default();
        let allow: DiscoveryPolicy = toml::from_str(
            r#"
            [allow]
            crates = { contested = "*" }
            "#,
        )
        .unwrap();
        let deny: DiscoveryPolicy = toml::from_str(
            r#"
            [deny]
            crates = { contested = "*" }
            "#,
        )
        .unwrap();
        collected.add_policy(&allow);
        collected.add_policy(&deny);

        assert_eq!(
            collected.evaluate(&crate_candidate("contested")),
            PolicyVerdict::Denied
        );
    }

    #[test]
    fn crates_wildcard_allow_does_not_match_without_crate_registry() {
        // A `crates = "*"` allow should not match non-crate candidates
        // (once we have path/git candidates). For now just verify it
        // works for crate candidates.
        let policy = policy_from_toml(
            r#"
            [allow]
            crates = "*"
            "#,
        );
        assert_eq!(
            policy.evaluate(&crate_candidate("any-crate")),
            PolicyVerdict::Allowed
        );
    }

    #[test]
    fn hyphen_underscore_normalization() {
        let policy = policy_from_toml(
            r#"
            [allow]
            crates = { serde_json = "*" }
            "#,
        );
        assert_eq!(
            policy.evaluate(&crate_candidate("serde-json")),
            PolicyVerdict::Allowed
        );
    }

    #[test]
    fn no_allow_rules_means_has_any_allow_rules_false() {
        let policy = CollectedPolicy::default();
        assert!(!policy.has_any_allow_rules());
    }

    #[test]
    fn with_allow_rules_reports_true() {
        let policy = policy_from_toml(r#"allow = "*""#);
        assert!(policy.has_any_allow_rules());
    }
}
