//! Convert one-pass predicate evidence into safe resolution witnesses.
//!
//! The predicate layer reports the exact successful branch with either one
//! admitted package or an ambiguity marker. This module is the privacy
//! boundary that applies public naming policy and replaces everything else
//! with fixed opaque nodes. It never re-evaluates a predicate and never sees
//! either candidate from an ambiguous match.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the structured sync report consumes resolution witnesses in the next commit"
    )
)]

use crate::{
    pm::PackageNamingView,
    predicate::{
        DependencyMatch, PredicateEvaluationMode, PredicateWitnessEval, ResolvedDependencies,
        ResolvedPredicateContext,
    },
};
use symposium_sdk::predicate::syntax::PredicateSet;

use super::{
    policy::{ExtensionNaming, PackageNaming},
    schema::ResolutionWitness,
};

/// Evaluate a predicate set once and retain only safe evidence from that pass.
///
/// A successful empty set returns an empty witness so its parent can flatten
/// the set without inventing a wire node.
pub(in crate::telemetry) fn evaluate_predicates<'a>(
    predicates: &PredicateSet,
    context: &mut ResolvedPredicateContext<'a>,
) -> Option<ResolutionWitness> {
    predicates.evaluate_witness(context, &mut SafeResolutionMode)
}

/// Convert one package naming decision into a safe path fragment.
#[must_use]
pub(in crate::telemetry) fn package(naming: &PackageNaming) -> ResolutionWitness {
    match naming {
        PackageNaming::Public(coordinate) => ResolutionWitness::package(coordinate.clone()),
        PackageNaming::Unnamed(_) => ResolutionWitness::private_source(),
    }
}

/// Convert one extension naming decision into a safe path fragment.
#[must_use]
pub(in crate::telemetry) fn extension(naming: &ExtensionNaming) -> ResolutionWitness {
    match naming {
        ExtensionNaming::Public(coordinate) => ResolutionWitness::extension(coordinate.clone()),
        ExtensionNaming::Unnamed => ResolutionWitness::private_source(),
    }
}

struct SafeResolutionMode;

impl<'a> PredicateEvaluationMode<ResolvedDependencies<'a>> for SafeResolutionMode {
    type Evidence = ResolutionWitness;
    const DISTINGUISH_DEPENDENCY_MATCHES: bool = true;

    fn dependency(
        &mut self,
        matched: DependencyMatch<'_, PackageNamingView<'a>>,
    ) -> Self::Evidence {
        match matched {
            DependencyMatch::Unique(view) => package(&super::policy::package(*view)),
            // Several packages may all be public. Ambiguity still cannot pick
            // one exact origin, so decide it before applying naming policy.
            DependencyMatch::Ambiguous => ResolutionWitness::private_source(),
        }
    }

    fn non_package(&mut self) -> Self::Evidence {
        ResolutionWitness::non_package_predicate()
    }

    fn not(&mut self) -> Self::Evidence {
        ResolutionWitness::not()
    }

    fn any(&mut self, child: Self::Evidence) -> Self::Evidence {
        ResolutionWitness::any(child)
    }

    fn all(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        ResolutionWitness::all(children)
    }

    fn set(&mut self, children: Vec<Self::Evidence>) -> Self::Evidence {
        let mut witness = ResolutionWitness::empty();
        for child in children {
            witness.append(child);
        }
        witness
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use symposium_sdk::{
        pm::{PackageId, PackageSourceClaim},
        predicate::syntax::{Dependency, Predicate},
    };

    use super::*;
    use crate::{
        pm::{CARGO_PM, PackageNamingAuthority, ResolvedDependency},
        telemetry::{policy, schema::UnnamedPackageReason},
    };

    fn dependency(
        slot: usize,
        name: &str,
        source: PackageSourceClaim,
        authority: PackageNamingAuthority,
    ) -> ResolvedDependency {
        ResolvedDependency::for_test(
            slot,
            PackageId::new(CARGO_PM, name, "1.2.3"),
            source,
            authority,
        )
    }

    fn crates_io(slot: usize, name: &str) -> ResolvedDependency {
        dependency(
            slot,
            name,
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
        )
    }

    fn predicate_set(name: &str) -> PredicateSet {
        PredicateSet {
            predicates: vec![Predicate::DependsOn(Dependency::named(name))],
        }
    }

    fn path_json(witness: ResolutionWitness) -> serde_json::Value {
        serde_json::to_value(
            witness
                .into_path()
                .expect("test witness should contain path evidence"),
        )
        .unwrap()
    }

    #[test]
    fn public_dependency_evidence_keeps_only_the_reviewed_coordinate() {
        let dependencies = [crates_io(0, "example-runtime")];
        let mut context = ResolvedPredicateContext::new(&dependencies);

        let witness = evaluate_predicates(&predicate_set("example-runtime"), &mut context).unwrap();

        assert_eq!(
            path_json(witness),
            json!([{
                "type": "package",
                "ecosystem": "cargo",
                "name": "example-runtime",
                "version": "1.2.3"
            }])
        );
    }

    #[test]
    fn private_dependency_evidence_is_opaque_and_content_free() {
        let private_name = "internal-runtime";
        let dependencies = [dependency(
            0,
            private_name,
            PackageSourceClaim::Registry,
            PackageNamingAuthority::Remote,
        )];
        let mut context = ResolvedPredicateContext::new(&dependencies);

        let encoded = serde_json::to_string(
            &evaluate_predicates(&predicate_set(private_name), &mut context)
                .unwrap()
                .into_path()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(encoded, r#"[{"type":"opaque","reason":"private_source"}]"#);
        assert!(!encoded.contains(private_name));
    }

    #[test]
    fn mixed_public_and_private_matches_are_opaque_in_either_order() {
        let public = crates_io(0, "example-runtime");
        let private = dependency(
            1,
            "example-runtime",
            PackageSourceClaim::Registry,
            PackageNamingAuthority::Remote,
        );

        for dependencies in [
            [public.clone(), private.clone()],
            [private.clone(), public.clone()],
        ] {
            let mut context = ResolvedPredicateContext::new(&dependencies);
            assert_eq!(
                path_json(
                    evaluate_predicates(&predicate_set("example-runtime"), &mut context).unwrap()
                ),
                json!([{ "type": "opaque", "reason": "private_source" }])
            );
        }
    }

    #[test]
    fn repeated_handle_remains_one_public_match() {
        let dependency = crates_io(0, "example-runtime");
        let dependencies = [dependency.clone(), dependency];
        let mut context = ResolvedPredicateContext::new(&dependencies);

        assert_eq!(
            path_json(
                evaluate_predicates(&predicate_set("example-runtime"), &mut context).unwrap()
            ),
            json!([{
                "type": "package",
                "ecosystem": "cargo",
                "name": "example-runtime",
                "version": "1.2.3"
            }])
        );
    }

    #[test]
    fn multiple_public_or_unnamed_matches_do_not_select_one_origin() {
        let cases = [
            [
                crates_io(0, "example-runtime"),
                crates_io(1, "example-runtime"),
            ],
            [
                dependency(
                    0,
                    "example-runtime",
                    PackageSourceClaim::Registry,
                    PackageNamingAuthority::Remote,
                ),
                dependency(
                    1,
                    "example-runtime",
                    PackageSourceClaim::Path,
                    PackageNamingAuthority::BuiltinCargo,
                ),
            ],
        ];

        for dependencies in cases {
            let mut context = ResolvedPredicateContext::new(&dependencies);
            assert_eq!(
                path_json(
                    evaluate_predicates(&predicate_set("example-runtime"), &mut context).unwrap()
                ),
                json!([{ "type": "opaque", "reason": "private_source" }])
            );
        }
    }

    #[test]
    fn predicate_sets_flatten_while_explicit_all_retains_its_node() {
        let dependencies = [crates_io(0, "example-runtime")];
        let mut context = ResolvedPredicateContext::new(&dependencies);
        context.set_workspace_member(true);
        let predicates = PredicateSet {
            predicates: vec![
                Predicate::DependsOn(Dependency::named("example-runtime")),
                Predicate::Any(vec![
                    Predicate::DependsOn(Dependency::named("missing")),
                    Predicate::WorkspaceMember,
                ]),
                Predicate::All(vec![
                    Predicate::DependsOnWildcard,
                    Predicate::Not(Box::new(Predicate::DependsOn(Dependency::named("missing")))),
                ]),
            ],
        };

        let witness = evaluate_predicates(&predicates, &mut context).unwrap();

        assert_eq!(
            path_json(witness),
            json!([
                {
                    "type": "package",
                    "ecosystem": "cargo",
                    "name": "example-runtime",
                    "version": "1.2.3"
                },
                {
                    "type": "any",
                    "child": {
                        "type": "opaque",
                        "reason": "non_package_predicate"
                    }
                },
                {
                    "type": "all",
                    "children": [
                        {
                            "type": "opaque",
                            "reason": "non_package_predicate"
                        },
                        { "type": "not" }
                    ]
                }
            ])
        );
    }

    #[test]
    fn empty_predicate_set_contributes_no_path_node() {
        let mut context = ResolvedPredicateContext::new(&[]);

        let witness = evaluate_predicates(&PredicateSet::default(), &mut context).unwrap();

        assert_eq!(witness.into_path(), None);
    }

    #[test]
    fn empty_all_cannot_emit_a_reader_invalid_node() {
        assert_eq!(
            path_json(ResolutionWitness::all(Vec::new())),
            json!([{
                "type": "opaque",
                "reason": "non_package_predicate"
            }])
        );
    }

    #[test]
    fn public_and_unnamed_extensions_use_the_same_safe_boundary() {
        let public_id = PackageId::new(CARGO_PM, "example-tools", "1.2.3");
        let public_view = PackageNamingView::for_test(
            &public_id,
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
        );
        let private_id = PackageId::new(CARGO_PM, "private-tools", "1.2.3");
        let private_view = PackageNamingView::for_test(
            &private_id,
            PackageSourceClaim::Path,
            PackageNamingAuthority::BuiltinCargo,
        );
        let public = policy::plugin(public_view, "example-debugging");
        let unnamed = policy::plugin(private_view, "private-debugging");

        assert_eq!(
            path_json(extension(&public)),
            json!([{
                "type": "extension",
                "extension_type": "plugin",
                "source": "crates-io",
                "name": "example-debugging"
            }])
        );
        assert_eq!(
            path_json(extension(&unnamed)),
            json!([{ "type": "opaque", "reason": "private_source" }])
        );
    }

    #[test]
    fn composition_preserves_package_predicate_and_extension_order() {
        let id = PackageId::new(CARGO_PM, "example-runtime", "1.2.3");
        let view = PackageNamingView::for_test(
            &id,
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
        );
        let package_naming = policy::package(view);
        let extension_naming = policy::plugin(view, "example-tools");
        let mut witness = package(&package_naming);
        witness.append(ResolutionWitness::not());
        witness.append(extension(&extension_naming));

        let encoded = serde_json::to_string(&witness.into_path().unwrap()).unwrap();

        assert_eq!(
            encoded,
            r#"[{"type":"package","ecosystem":"cargo","name":"example-runtime","version":"1.2.3"},{"type":"not"},{"type":"extension","extension_type":"plugin","source":"crates-io","name":"example-tools"}]"#
        );
    }

    #[test]
    fn depth_leaf_and_encoded_size_limits_degrade_to_opaque_limit() {
        let mut nested = Predicate::DependsOnWildcard;
        for _ in 0..9 {
            nested = Predicate::Any(vec![nested]);
        }
        let depth_set = PredicateSet {
            predicates: vec![nested],
        };
        let leaf_set = PredicateSet {
            predicates: vec![Predicate::DependsOnWildcard; 17],
        };
        let oversized_id = PackageId::new(
            CARGO_PM,
            "example-runtime",
            format!("1.2.3+{}", "a".repeat(4_096)),
        );
        let oversized = policy::package(PackageNamingView::for_test(
            &oversized_id,
            PackageSourceClaim::CratesIo,
            PackageNamingAuthority::BuiltinCargo,
        ));

        let mut empty_context = ResolvedPredicateContext::new(&[]);
        let depth_json = path_json(evaluate_predicates(&depth_set, &mut empty_context).unwrap());
        let leaf_json = path_json(evaluate_predicates(&leaf_set, &mut empty_context).unwrap());
        let size_json = path_json(package(&oversized));

        assert!(depth_json.to_string().contains(r#""reason":"limit""#));
        assert_eq!(leaf_json, json!([{ "type": "opaque", "reason": "limit" }]));
        assert_eq!(size_json, json!([{ "type": "opaque", "reason": "limit" }]));
    }

    #[test]
    fn every_unnamed_package_reason_uses_the_same_opaque_node() {
        for reason in [
            UnnamedPackageReason::PrivateRegistry,
            UnnamedPackageReason::Git,
            UnnamedPackageReason::Path,
            UnnamedPackageReason::Workspace,
            UnnamedPackageReason::UnknownSource,
            UnnamedPackageReason::InvalidCoordinate,
        ] {
            assert_eq!(
                path_json(package(&PackageNaming::Unnamed(reason))),
                json!([{ "type": "opaque", "reason": "private_source" }])
            );
        }
    }
}
