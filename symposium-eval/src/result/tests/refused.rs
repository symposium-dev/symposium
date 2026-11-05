//! Parts that cannot belong to one trial, which a result refuses.

use std::io;

use super::{execution, expectations, graded, passing, try_result_of};
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
