//! Parts that cannot belong to one trial, which a result refuses.

use std::io;

use super::{execution, expectations, graded, not_launched, passing, try_result_of};
use crate::client::{Ended, Load, Usage};
use crate::result::{Reached, ResultError, SetupStep};
use crate::run::Configuration;

#[test]
fn expectations_must_match_the_configuration() {
    let execution = execution(Ended::Completed, Usage::Unknown);
    let (grading, expectations) = (passing(), expectations());

    let baseline = try_result_of(
        Configuration::Baseline,
        graded(&execution, &grading, Some(&expectations)),
    );
    let symposium = try_result_of(Configuration::Symposium, graded(&execution, &grading, None));

    assert_eq!(
        baseline.unwrap_err(),
        ResultError::Expectations {
            configuration: Configuration::Baseline,
            expectations: true,
        }
    );
    assert_eq!(
        symposium.unwrap_err(),
        ResultError::Expectations {
            configuration: Configuration::Symposium,
            expectations: false,
        }
    );
}

#[test]
fn a_symposium_trial_stopped_before_launch_needs_no_expectations() {
    let error = io::Error::other("init failed");
    let reached = Reached::Setup {
        step: SetupStep::Setup,
        error: &error,
    };

    assert!(try_result_of(Configuration::Symposium, reached).is_ok());
}

#[test]
fn loads_must_match_the_skills_looked_for() {
    for loads in [Vec::new(), vec![Load::Loaded, Load::Unknown]] {
        let mut execution = execution(Ended::Completed, Usage::Unknown);
        execution.loads = loads.clone();
        let grading = passing();

        let result = try_result_of(Configuration::Baseline, graded(&execution, &grading, None));

        assert_eq!(
            result.unwrap_err(),
            ResultError::Loads {
                named: 1,
                observed: loads.len(),
            }
        );
    }
}

#[test]
fn only_a_client_that_never_launched_goes_ungraded() {
    let (launched, unlaunched) = (execution(Ended::Completed, Usage::Unknown), not_launched());
    let grading = passing();
    let session = |execution| Reached::Session {
        execution,
        changes: &[],
    };
    let result = |reached| try_result_of(Configuration::Baseline, reached);

    assert!(result(session(&unlaunched)).is_ok());
    assert!(result(graded(&launched, &grading, None)).is_ok());
    assert_eq!(
        result(session(&launched)).unwrap_err(),
        ResultError::SessionPhase { launched: true }
    );
    assert_eq!(
        result(graded(&unlaunched, &grading, None)).unwrap_err(),
        ResultError::SessionPhase { launched: false }
    );
}

#[test]
fn uncertain_containment_has_its_own_ungraded_phase() {
    let grading = passing();
    for ended in [
        Ended::Unsupervised(crate::grade::ProcessError::OutputNotClosed),
        Ended::TimedOut {
            after_deadline: Some(crate::grade::ProcessError::OutputNotClosed),
        },
    ] {
        let execution = execution(ended, Usage::Unknown);
        let reached = Reached::Uncontained {
            execution: &execution,
        };
        for configuration in [Configuration::Baseline, Configuration::Symposium] {
            assert!(try_result_of(configuration, reached).is_ok());
        }
        assert_eq!(
            try_result_of(Configuration::Baseline, graded(&execution, &grading, None)).unwrap_err(),
            ResultError::SessionContainment { certain: false }
        );
    }
    let contained = execution(Ended::Completed, Usage::Unknown);
    assert_eq!(
        try_result_of(
            Configuration::Baseline,
            Reached::Uncontained {
                execution: &contained
            }
        )
        .unwrap_err(),
        ResultError::SessionContainment { certain: true }
    );
}
