//! Deciding a task's `[expected]` assertions from what a trial left (RFD reference,
//! "Expectations"). Evidence that cannot decide an assertion makes it
//! [`Verdict::Inconclusive`], never quietly a pass or a fail.

pub(crate) mod config;
pub(crate) mod crosscheck;
pub(crate) mod file;
pub(crate) mod hooks;
pub(crate) mod installed;
pub(crate) mod log;
pub(crate) mod sync;
pub(crate) mod trace;

use crate::client::Load;
use crate::task::{SkillExpectations, SkillName};

/// How the evidence decides one assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(crate) enum Verdict {
    Pass,
    Fail,
    Inconclusive,
}

/// Which of the two use assertions a skill is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UseAssertion {
    Used,
    NotUsed,
}

/// One skill's use assertion, decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkillUse {
    pub(crate) skill: SkillName,
    pub(crate) assertion: UseAssertion,
    /// Reported whatever the verdict, since a failed attempt passes `not_used`.
    pub(crate) load: Load,
    pub(crate) verdict: Verdict,
}

impl UseAssertion {
    /// The RFD's table: a failed or denied attempt never delivered the skill's content,
    /// so it fails `used` and passes `not_used`.
    fn decide(self, load: Load) -> Verdict {
        match (self, load) {
            (_, Load::Unknown) => Verdict::Inconclusive,
            (Self::Used, Load::Loaded) | (Self::NotUsed, Load::Attempted | Load::NotObserved) => {
                Verdict::Pass
            }
            (Self::Used, Load::Attempted | Load::NotObserved) | (Self::NotUsed, Load::Loaded) => {
                Verdict::Fail
            }
        }
    }
}

/// Decides `expected`'s `used` and `not_used` lists, with `load_of` giving what the
/// transcript showed for each named skill.
pub(crate) fn skill_use(
    expected: &SkillExpectations,
    load_of: impl Fn(&SkillName) -> Load,
) -> Vec<SkillUse> {
    let named = |assertion, skills: &[SkillName]| {
        skills
            .iter()
            .map(|skill| {
                let load = load_of(skill);
                SkillUse {
                    skill: skill.clone(),
                    assertion,
                    load,
                    verdict: UseAssertion::decide(assertion, load),
                }
            })
            .collect::<Vec<_>>()
    };
    let mut decided = named(UseAssertion::Used, expected.used());
    decided.extend(named(UseAssertion::NotUsed, expected.not_used()));
    decided
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOADS: [Load; 4] = [
        Load::Loaded,
        Load::Attempted,
        Load::NotObserved,
        Load::Unknown,
    ];

    #[test]
    fn used_passes_only_on_a_load() {
        let verdicts = LOADS.map(|load| UseAssertion::Used.decide(load));

        assert_eq!(
            verdicts,
            [
                Verdict::Pass,
                Verdict::Fail,
                Verdict::Fail,
                Verdict::Inconclusive
            ]
        );
    }

    #[test]
    fn not_used_fails_only_on_a_load() {
        let verdicts = LOADS.map(|load| UseAssertion::NotUsed.decide(load));

        assert_eq!(
            verdicts,
            [
                Verdict::Fail,
                Verdict::Pass,
                Verdict::Pass,
                Verdict::Inconclusive
            ]
        );
    }

    fn skill(name: &str) -> SkillName {
        SkillName::new(name).unwrap()
    }

    #[test]
    fn each_named_skill_is_decided_with_its_own_load() {
        let expected =
            SkillExpectations::using(vec![skill("first-skill")], vec![skill("second-skill")]);

        let decided = skill_use(&expected, |name| {
            if *name == skill("first-skill") {
                Load::Loaded
            } else {
                Load::Attempted
            }
        });

        assert_eq!(
            decided,
            [
                SkillUse {
                    skill: skill("first-skill"),
                    assertion: UseAssertion::Used,
                    load: Load::Loaded,
                    verdict: Verdict::Pass,
                },
                SkillUse {
                    skill: skill("second-skill"),
                    assertion: UseAssertion::NotUsed,
                    load: Load::Attempted,
                    verdict: Verdict::Pass,
                },
            ]
        );
    }
}
